// src/lexer/mesin.rs — mesin tokenisasi

#![allow(dead_code, unused_variables, unused_imports)]

use unicode_xid::UnicodeXID;
use super::*;

impl<'src> Lexer<'src> {
    pub(crate) fn new(sf: &'src SourceFile) -> Self {
        Lexer {
            src:         &sf.src,
            file_id:     sf.id,
            pos:         0,
            chars:       sf.src.chars().collect(),
            char_pos:    0,
            tmpl_stack:  Vec::new(),
            pending:     std::collections::VecDeque::new(),
            comment_depth: 0,
            last_token_auto_semi: false,
            komentar:    Vec::new(),
        }
    }

    // ── Core character operations ─────────────────────────────────────────

    pub(crate) fn current(&self) -> Option<char> {
        self.chars.get(self.char_pos).copied()
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.chars.get(self.char_pos + 1).copied()
    }

    pub(crate) fn peek2(&self) -> Option<char> {
        self.chars.get(self.char_pos + 2).copied()
    }

    pub(crate) fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.char_pos).copied();
        if let Some(c) = ch {
            self.char_pos += 1;
            self.pos += c.len_utf8();
        }
        ch
    }

    pub(crate) fn byte_pos(&self) -> BytePos {
        BytePos(self.pos as u32)
    }

    pub(crate) fn span_from(&self, lo: BytePos) -> Span {
        Span::new(lo, self.byte_pos(), self.file_id)
    }

    pub(crate) fn skip_whitespace_no_newline(&mut self) {
        while let Some(c) = self.current() {
            if c == ' ' || c == '\t' || c == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    // ── Automatic semicolon look-ahead ──────────────────────────────────────
    pub(crate) fn berikutnya_menahan_titik_koma(&self) -> bool {
        let mut i = self.char_pos;
        loop {
            match self.chars.get(i).copied() {
                None => return false,
                Some(c) if c.is_whitespace() => {
                    i += 1;
                }
                Some('/') if self.chars.get(i + 1).copied() == Some('/') => {
                    // Line comment: skip to (but not past) the newline that ends it.
                    i += 2;
                    loop {
                        match self.chars.get(i).copied() {
                            None => return false,
                            Some('\n') => break,
                            Some(_) => { i += 1; }
                        }
                    }
                }
                Some('/') if self.chars.get(i + 1).copied() == Some('*') => {
                    // Block comment: skip past the matching, possibly nested, `*/`.
                    i += 2;
                    let mut depth: u32 = 1;
                    loop {
                        match self.chars.get(i).copied() {
                            None => return false, // unterminated; let real lexing report it
                            Some('/') if self.chars.get(i + 1).copied() == Some('*') => {
                                depth += 1;
                                i += 2;
                            }
                            Some('*') if self.chars.get(i + 1).copied() == Some('/') => {
                                depth -= 1;
                                i += 2;
                                if depth == 0 { break; }
                            }
                            Some(_) => { i += 1; }
                        }
                    }
                }
                Some('}') | Some(')') | Some(']') => return true,

                // Token yang MUSTAHIL memulai sebuah statement.
                Some('.') | Some('?') | Some('$') | Some(':')
                | Some('+') | Some('/') | Some('%') | Some('^')
                | Some('=') | Some('>') | Some(',') => return true,

                // `<` boleh, `<-` TIDAK — dan ini pengecualian yang sempat hilang.
                Some('<') => return self.chars.get(i + 1).copied() != Some('-'),

                // `else` di baris sendiri — cara paling lumrah memformat if/else di seluruh
                // bahasa keluarga C, dan sampai sekarang DITOLAK:
                Some('e') if self.kata_di(i) == Some(&['e', 'l', 's', 'e'][..]) => return true,

                // `&&` boleh, `&` sendirian tidak (`&x` itu referensi).
                Some('&') => return self.chars.get(i + 1).copied() == Some('&'),
                // `!=` boleh, `!` sendirian tidak (`!x` itu negasi logika).
                Some('!') => return self.chars.get(i + 1).copied() == Some('='),

                Some(_) => return false,
            }
        }
    }

    /// Kata identifier utuh yang mulai di posisi `i`, atau `None` kalau di situ
    /// bukan awal identifier.
    pub(crate) fn kata_di(&self, i: usize) -> Option<&[char]> {
        let mut j = i;
        while let Some(c) = self.chars.get(j).copied() {
            if UnicodeXID::is_xid_continue(c) { j += 1; } else { break; }
        }
        if j == i { return None; }
        Some(&self.chars[i..j])
    }

    // ── Comment handling ─────────────────────────────────────────────────

    pub(crate) fn skip_line_comment(&mut self) {
        // Already consumed "//"
        while let Some(c) = self.current() {
            if c == '\n' { break; }
            self.advance();
        }
    }

    pub(crate) fn skip_block_comment(&mut self) -> Result<(), LexError> {
        // Already consumed "/*"
        let lo = self.byte_pos();
        let mut depth = 1u32;
        loop {
            match self.current() {
                None => {
                    return Err(LexError {
                        msg: "komentar blok `/*` belum ditutup — kurang `*/`".to_string(),
                        span: self.span_from(lo),
                    });
                }
                Some('/') if self.peek() == Some('*') => {
                    self.advance(); self.advance();
                    depth += 1;
                }
                Some('*') if self.peek() == Some('/') => {
                    self.advance(); self.advance();
                    depth -= 1;
                    if depth == 0 { break; }
                }
                Some(_) => { self.advance(); }
            }
        }
        Ok(())
    }

    // ── Number lexing ─────────────────────────────────────────────────────

    pub(crate) fn lex_number(&mut self, lo: BytePos) -> Result<Token, LexError> {
        let mut is_float   = false;
        let mut is_imag    = false;
        let mut text       = String::new();
        let mut radix: u32 = 10;

        // Detect base prefix
        if self.current() == Some('0') {
            match self.peek() {
                Some('x') | Some('X') => {
                    text.push('0');
                    self.advance();
                    text.push(self.advance().unwrap());
                    radix = 16;
                }
                Some('o') | Some('O') => {
                    text.push('0');
                    self.advance();
                    text.push(self.advance().unwrap());
                    radix = 8;
                }
                Some('b') | Some('B') => {
                    text.push('0');
                    self.advance();
                    text.push(self.advance().unwrap());
                    radix = 2;
                }
                _ => {}
            }
        }

        // Digits (skip visual-separator underscores)
        loop {
            match self.current() {
                Some('_') => { self.advance(); }
                Some(c) if c.is_digit(radix) => {
                    text.push(c); self.advance();
                }
                _ => break,
            }
        }

        // Fractional part (decimal/hex only)
        if radix == 10 || radix == 16 {
            if self.current() == Some('.') && self.peek().map_or(false, |c| c.is_digit(radix)) {
                is_float = true;
                text.push('.');
                self.advance();
                loop {
                    match self.current() {
                        Some('_') => { self.advance(); }
                        Some(c) if c.is_digit(radix) => { text.push(c); self.advance(); }
                        _ => break,
                    }
                }
            }
        }

        // Exponent
        match self.current() {
            Some('e') | Some('E') if radix == 10 => {
                is_float = true;
                text.push('e');
                self.advance();
                if let Some(sign) = self.current() {
                    if sign == '+' || sign == '-' {
                        text.push(sign);
                        self.advance();
                    }
                }
                while let Some(c) = self.current() {
                    if c.is_ascii_digit() { text.push(c); self.advance(); }
                    else { break; }
                }
            }
            Some('p') | Some('P') if radix == 16 => {
                is_float = true;
                text.push('p');
                self.advance();
                if let Some(sign) = self.current() {
                    if sign == '+' || sign == '-' { text.push(sign); self.advance(); }
                }
                while let Some(c) = self.current() {
                    if c.is_ascii_digit() { text.push(c); self.advance(); }
                    else { break; }
                }
            }
            _ => {}
        }

        // Imaginary suffix
        if self.current() == Some('i') {
            is_imag = true;
            self.advance();
        }

        let span = self.span_from(lo);
        if is_imag {
            let val: f64 = text.parse().map_err(|_| LexError {
                msg: format!("angka imajiner `{}i` tidak sah", text),
                span,
            })?;
            return Ok(Token::new(TokenKind::ImaginaryLit(val), span));
        }
        if is_float {
            let val: f64 = text.parse().map_err(|_| LexError {
                msg: format!("angka pecahan `{}` tidak sah", text),
                span,
            })?;
            return Ok(Token::new(TokenKind::FloatLit(val), span));
        }
        // Integer
        let val = u128::from_str_radix(&text, radix).map_err(|_| LexError {
            msg: format!("angka bulat `{}` tidak sah (basis {})", text, radix),
            span,
        })?;
        Ok(Token::new(TokenKind::IntLit(val), span))
    }

    // ── String lexing ─────────────────────────────────────────────────────

    pub(crate) fn lex_string(&mut self, lo: BytePos) -> Result<Token, LexError> {
        // Opening '"' already consumed
        let mut s = String::new();
        loop {
            match self.current() {
                None | Some('\n') => {
                    return Err(LexError {
                        msg: "string belum ditutup — kurang tanda kutip penutup".to_string(),
                        span: self.span_from(lo),
                    });
                }
                Some('"') => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    self.advance();
                    let esc = self.lex_escape(lo)?;
                    s.push(esc);
                }
                Some(c) => {
                    s.push(c);
                    self.advance();
                }
            }
        }
        Ok(Token::new(TokenKind::StringLit(s), self.span_from(lo)))
    }

    pub(crate) fn lex_raw_string(&mut self, lo: BytePos) -> Result<Token, LexError> {
        // Opening '`' already consumed
        let mut s = String::new();
        loop {
            match self.current() {
                None => {
                    return Err(LexError {
                        msg: "string mentah belum ditutup — kurang backtick penutup".to_string(),
                        span: self.span_from(lo),
                    });
                }
                Some('`') => {
                    self.advance();
                    break;
                }
                Some(c) => {
                    s.push(c);
                    self.advance();
                }
            }
        }
        Ok(Token::new(TokenKind::RawStringLit(s), self.span_from(lo)))
    }

    pub(crate) fn lex_rune(&mut self, lo: BytePos) -> Result<Token, LexError> {
        // Opening '\'' already consumed
        let ch = match self.current() {
            None | Some('\n') => {
                return Err(LexError {
                    msg: "rune belum ditutup — kurang tanda kutip tunggal penutup".to_string(),
                    span: self.span_from(lo),
                });
            }
            Some('\\') => {
                self.advance();
                self.lex_escape(lo)?
            }
            Some(c) => {
                self.advance();
                c
            }
        };
        match self.current() {
            Some('\'') => { self.advance(); }
            _ => {
                return Err(LexError {
                    msg: "rune cuma boleh satu karakter — pakai tanda kutip ganda kalau maksudnya string".to_string(),
                    span: self.span_from(lo),
                });
            }
        }
        Ok(Token::new(TokenKind::RuneLit(ch), self.span_from(lo)))
    }

    pub(crate) fn lex_escape(&mut self, lo: BytePos) -> Result<char, LexError> {
        match self.advance() {
            Some('a')  => Ok('\x07'),
            Some('b')  => Ok('\x08'),
            Some('f')  => Ok('\x0C'),
            Some('n')  => Ok('\n'),
            Some('r')  => Ok('\r'),
            Some('t')  => Ok('\t'),
            Some('v')  => Ok('\x0B'),
            Some('\\') => Ok('\\'),
            Some('\'') => Ok('\''),
            Some('"')  => Ok('"'),
            // Pembatas template bisa di-escape supaya bisa ditulis
            // sebagai teks biasa di dalam `«...»`.
            Some('«')  => Ok('«'),
            Some('»')  => Ok('»'),
            Some('$')  => Ok('$'),
            Some('x')  => {
                let h1 = self.advance().and_then(|c| c.to_digit(16));
                let h2 = self.advance().and_then(|c| c.to_digit(16));
                match (h1, h2) {
                    (Some(a), Some(b)) => {
                        char::from_u32(a * 16 + b).ok_or_else(|| LexError {
                            msg: "escape `\\x` tidak sah".to_string(),
                            span: self.span_from(lo),
                        })
                    }
                    _ => Err(LexError {
                        msg: "`\\x` butuh dua digit heksadesimal sesudahnya, misalnya `\\x41`".to_string(),
                        span: self.span_from(lo),
                    }),
                }
            }
            Some('u') => self.lex_unicode_escape(4, lo),
            Some('U') => self.lex_unicode_escape(8, lo),
            Some(c) => Err(LexError {
                msg: format!("escape `\\{}` tidak dikenal", c),
                span: self.span_from(lo),
            }),
            None => Err(LexError {
                msg: "berkasnya habis di tengah escape sequence".to_string(),
                span: self.span_from(lo),
            }),
        }
    }

    pub(crate) fn lex_unicode_escape(&mut self, digits: usize, lo: BytePos) -> Result<char, LexError> {
        let mut codepoint: u32 = 0;
        for _ in 0..digits {
            match self.advance().and_then(|c| c.to_digit(16)) {
                Some(d) => codepoint = codepoint * 16 + d,
                None => {
                    return Err(LexError {
                        msg: format!("escape Unicode butuh {} digit heksadesimal", digits),
                        span: self.span_from(lo),
                    });
                }
            }
        }
        char::from_u32(codepoint).ok_or_else(|| LexError {
            msg: format!("codepoint Unicode U+{:04X} tidak sah", codepoint),
            span: self.span_from(lo),
        })
    }

    // ── Template literal lex (state: inside «...») ────────────────────────

    // Template literals are complex because they contain nested expressions.

    pub(crate) fn lex_template_body(&mut self, lo: BytePos) -> Result<Token, LexError> {
        // Penutup mana yang sedang ditunggu. Dua pembuka (`«` dan `"""`) tapi
        // SATU badan: kalau keduanya punya jalur sendiri, keduanya akan
        // melenceng — persis kelas salinan-kedua yang berulang di proyek ini.
        let kutip_tiga = matches!(
            self.tmpl_stack.last(), Some(TmplState::Text { kutip_tiga: true }));
        let mut s = String::new();
        loop {
            // Penutup `"""`. Diperiksa lebih dulu supaya sebuah `"` tunggal
            // di dalamnya tetap teks biasa — itu yang membuat menulis JSON
            // atau HTML di dalamnya nyaman.
            if kutip_tiga
                && self.current() == Some('"')
                && self.peek() == Some('"')
                && self.peek2() == Some('"')
            {
                self.advance(); self.advance(); self.advance();
                self.tmpl_stack.pop();
                let span = self.span_from(lo);
                let end = Token::new(TokenKind::TemplateLitEnd, span);
                if s.is_empty() {
                    return Ok(end);
                }
                self.pending.push_back(end);
                return Ok(Token::new(TokenKind::StringLit(s), span));
            }
            match self.current() {
                None => {
                    return Err(LexError {
                        msg: if kutip_tiga {
                            "template `\"\"\"...\"\"\"` belum ditutup — file keburu habis. \
                             Tambahkan `\"\"\"` di ujungnya".to_string()
                        } else {
                            "template literal `«...»` belum ditutup — file keburu habis. \
                             Tambahkan `»` di ujung template ini".to_string()
                        },
                        span: self.span_from(lo),
                    });
                }
                // Penutup `»`.
                Some('»') if !kutip_tiga => {
                    self.advance();
                    self.tmpl_stack.pop();
                    let span = self.span_from(lo);
                    let end = Token::new(TokenKind::TemplateLitEnd, span);
                    if s.is_empty() {
                        return Ok(end);
                    }
                    self.pending.push_back(end);
                    return Ok(Token::new(TokenKind::StringLit(s), span));
                }
                // `${` membuka interpolasi: mulai sekarang token dilex
                // sebagai ekspresi biasa sampai `}` pasangannya.
                Some('$') if self.peek() == Some('{') => {
                    self.advance(); self.advance(); // '$' lalu '{'
                    self.tmpl_stack.push(TmplState::Interp(0));
                    let span = self.span_from(lo);
                    let interp = Token::new(TokenKind::TemplateInterpStart, span);
                    if s.is_empty() {
                        return Ok(interp);
                    }
                    self.pending.push_back(interp);
                    return Ok(Token::new(TokenKind::StringLit(s), span));
                }
                Some('\\') => {
                    self.advance();
                    let esc = self.lex_escape(lo)?;
                    s.push(esc);
                }
                Some(c) => {
                    s.push(c);
                    self.advance();
                }
            }
        }
    }

    // ── Operator / punctuation dispatch ───────────────────────────────────

    pub(crate) fn lex_op(&mut self, c: char, lo: BytePos) -> Result<Token, LexError> {
        let kind = match c {
            '+' => match self.current() {
                Some('=') => { self.advance(); TokenKind::PlusEq }
                _         => TokenKind::Plus,
            },
            '-' => match self.current() {
                Some('=') => { self.advance(); TokenKind::MinusEq }
                Some('>') => { self.advance(); TokenKind::Arrow }
                _         => TokenKind::Minus,
            },
            '*' => match self.current() {
                Some('=') => { self.advance(); TokenKind::StarEq }
                Some('*') => {
                    self.advance();
                    // `**=` melengkapi keluarga operator gabungan: `*=` dan
                    // `**` sudah ada, jadi ketiadaannya cuma lubang.
                    if self.current() == Some('=') { self.advance(); TokenKind::StarStarEq }
                    else { TokenKind::StarStar }
                }
                _         => TokenKind::Star,
            },
            '/' => match self.current() {
                Some('=') => { self.advance(); TokenKind::SlashEq }
                // `lo` di sini adalah posisi `/` pertama, jadi rentang yang
                // dicatat memuat pembukanya juga (`//` atau `/*`) — formatter
                // menyalin potongan ini apa adanya, tanpa merakit ulang.
                Some('/') => {
                    self.advance();
                    self.skip_line_comment();
                    let span = self.span_from(lo);
                    self.komentar.push(Komentar { span, blok: false });
                    return self.next_token();
                }
                Some('*') => {
                    self.advance();
                    self.skip_block_comment()?;
                    let span = self.span_from(lo);
                    self.komentar.push(Komentar { span, blok: true });
                    return self.next_token();
                }
                _         => TokenKind::Slash,
            },
            '%' => match self.current() {
                Some('=') => { self.advance(); TokenKind::PercentEq }
                _         => TokenKind::Percent,
            },
            '&' => match self.current() {
                Some('&') => { self.advance(); TokenKind::AndAnd }
                Some('=') => { self.advance(); TokenKind::AndEq }
                Some('^') => { self.advance(); TokenKind::AndCaret }
                Some('m') if self.peek() == Some('u') => {
                    // &mut
                    self.advance(); // m
                    self.advance(); // u
                    if self.current() == Some('t') { self.advance(); }
                    TokenKind::AmpMut
                }
                _         => TokenKind::Amp,
            },
            '|' => match self.current() {
                Some('|') => { self.advance(); TokenKind::OrOr }
                Some('=') => { self.advance(); TokenKind::OrEq }
                Some('>') => { self.advance(); TokenKind::Pipe }
                _         => TokenKind::Or,
            },
            '^' => match self.current() {
                Some('=') => { self.advance(); TokenKind::CaretEq }
                _         => TokenKind::Caret,
            },
            '~' => TokenKind::Tilde,
            '<' => match self.current() {
                Some('<') => {
                    self.advance();
                    if self.current() == Some('=') { self.advance(); TokenKind::ShlEq }
                    else { TokenKind::Shl }
                }
                Some('=') => { self.advance(); TokenKind::LtEq }
                Some('-') => { self.advance(); TokenKind::LArrow }
                _         => TokenKind::Lt,
            },
            '>' => match self.current() {
                Some('>') => {
                    self.advance();
                    if self.current() == Some('=') { self.advance(); TokenKind::ShrEq }
                    else { TokenKind::Shr }
                }
                Some('=') => { self.advance(); TokenKind::GtEq }
                _         => TokenKind::Gt,
            },
            '=' => match self.current() {
                Some('=') => { self.advance(); TokenKind::EqEq }
                Some('>') => { self.advance(); TokenKind::FatArrow }
                _         => TokenKind::Eq,
            },
            '!' => match self.current() {
                Some('=') => { self.advance(); TokenKind::BangEq }
                _         => TokenKind::Bang,
            },
            ':' => match self.current() {
                Some(':') => { self.advance(); TokenKind::ColonColon }
                Some('=') => { self.advance(); TokenKind::ColonEq }
                _         => TokenKind::Colon,
            },
            '.' => match self.current() {
                Some('.') => {
                    self.advance(); // consume second '.'
                    if self.current() == Some('.') {
                        self.advance(); // consume third '.'
                        TokenKind::DotDotDot          // ...
                    } else if self.current() == Some('=') {
                        self.advance(); // consume '='
                        TokenKind::DotDotEq           // ..=
                    } else {
                        TokenKind::DotDot             // ..
                    }
                }
                _         => TokenKind::Dot,          // single .
            },
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            // Di dalam `${ ...
            '{' => {
                if let Some(TmplState::Interp(depth)) = self.tmpl_stack.last_mut() {
                    *depth += 1;
                }
                TokenKind::LBrace
            }
            '}' => {
                match self.tmpl_stack.last_mut() {
                    Some(TmplState::Interp(0)) => {
                        self.tmpl_stack.pop();
                        TokenKind::TemplateInterpEnd
                    }
                    Some(TmplState::Interp(depth)) => { *depth -= 1; TokenKind::RBrace }
                    _ => TokenKind::RBrace,
                }
            }
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semi,
            '@' => TokenKind::At,
            '#' => TokenKind::Hash,
            // `??` sebelum `?`, kalau tidak `a ?? b` terbaca sebagai dua
            // propagasi error berturut-turut.
            '?' if self.current() == Some('?') => { self.advance(); TokenKind::QuestionQuestion }
            '?' => TokenKind::Question,
            // Pembuka ternary: `cond $ a : b`.
            '$' => TokenKind::Dollar,
            // `»` ketemu saat masih di dalam `${ ...
            '»' if matches!(self.tmpl_stack.last(), Some(TmplState::Interp(_))) => {
                return Err(LexError {
                    msg: "`${` di dalam template belum ditutup `}` — tutup dulu interpolasinya \
                          sebelum mengakhiri template dengan `»`".to_string(),
                    span: self.span_from(lo),
                });
            }
            other => {
                return Err(LexError {
                    msg: format!("karakter `{}` tidak dikenal di sini", other),
                    span: self.span_from(lo),
                });
            }
        };
        Ok(Token::new(kind, self.span_from(lo)))
    }

    // ── Main dispatch ─────────────────────────────────────────────────────

    pub(crate) fn next_token(&mut self) -> Result<Token, LexError> {
        // Token yang sudah diantrekan (lihat `pending`) selalu didahulukan.
        if let Some(tok) = self.pending.pop_front() {
            return Ok(tok);
        }

        // Di dalam bagian TEKS sebuah template (`«...»`, di luar `${...}`),
        // spasi dan newline adalah bagian dari string — jadi jangan
        // di-skip, dan jangan dilex sebagai token biasa.
        if matches!(self.tmpl_stack.last(), Some(TmplState::Text { .. })) {
            let lo = self.byte_pos();
            return self.lex_template_body(lo);
        }

        self.skip_whitespace_no_newline();
        let lo = self.byte_pos();

        match self.advance() {
            None => Ok(Token::new(TokenKind::Eof, self.span_from(lo))),

            Some('\n') => {
                // Auto-semicolon insertion — but never immediately before a closing `}`,
                // `)`, or `]` (see `next_significant_is_closer`), so multi-line tail …
                if self.last_token_auto_semi && !self.berikutnya_menahan_titik_koma() {
                    Ok(Token::new(TokenKind::Semi, self.span_from(lo)))
                } else {
                    self.next_token() // skip non-significant newline
                }
            }

            // ── Identifiers and keywords ──────────────────────────────────
            Some(c) if UnicodeXID::is_xid_start(c) || c == '_' => {
                let mut ident = String::new();
                ident.push(c);
                // Identifier berhenti di karakter non-identifier.
                while let Some(nc) = self.current() {
                    if UnicodeXID::is_xid_continue(nc) {
                        ident.push(nc);
                        self.advance();
                    } else {
                        break;
                    }
                }
                let kind = keyword_or_ident(&ident);
                Ok(Token::new(kind, self.span_from(lo)))
            }

            // ── Rune literal `'c'` atau label lifetime `'nama` ────────────
            Some('\'') => {
                match (self.current(), self.peek()) {
                    (Some(c), Some('\'')) if UnicodeXID::is_xid_start(c) => self.lex_rune(lo),
                    (Some(c), _) if UnicodeXID::is_xid_start(c) => {
                        let mut label = String::new();
                        label.push('\'');
                        while let Some(nc) = self.current() {
                            if UnicodeXID::is_xid_continue(nc) {
                                label.push(nc);
                                self.advance();
                            } else { break; }
                        }
                        Ok(Token::new(TokenKind::Ident(label), self.span_from(lo)))
                    }
                    _ => self.lex_rune(lo),
                }
            }

            // ── Literals ──────────────────────────────────────────────────
            Some(c) if c.is_ascii_digit() => {
                // Put back by adjusting: since we already advanced, we undo by
                // manipulating char_pos (safe because we know c is ascii).
                // Actually re-use the digit we consumed:
                let number_start = lo;
                // Re-lex from lo by reconstructing — we need to handle the
                // first digit. We do this by pre-populating the digit.
                // Re-entry: lex_number_from_char:
                self.lex_number_with_first_char(c, lo)
            }
            // `"""` membuka template; `"` tunggal tetap string biasa.
            Some('"') if self.current() == Some('"') && self.peek() == Some('"') => {
                self.advance(); self.advance(); // dua `"` sisanya
                self.tmpl_stack.push(TmplState::Text { kutip_tiga: true });
                Ok(Token::new(TokenKind::TemplateLitStart, self.span_from(lo)))
            }
            Some('"')  => self.lex_string(lo),
            Some('`')  => self.lex_raw_string(lo),
            Some('«')  => {
                self.tmpl_stack.push(TmplState::Text { kutip_tiga: false });
                Ok(Token::new(TokenKind::TemplateLitStart, self.span_from(lo)))
            }

            // ── Operators and punctuation ─────────────────────────────────
            Some(c) => self.lex_op(c, lo),
        }
    }

    pub(crate) fn lex_number_with_first_char(&mut self, first: char, lo: BytePos) -> Result<Token, LexError> {
        let mut is_float   = false;
        let mut is_imag    = false;
        let mut text       = String::new();
        let mut radix: u32 = 10;

        text.push(first);

        if first == '0' {
            match self.current() {
                Some('x') | Some('X') => {
                    text.push(self.advance().unwrap());
                    radix = 16;
                }
                Some('o') | Some('O') => {
                    text.push(self.advance().unwrap());
                    radix = 8;
                }
                Some('b') | Some('B') => {
                    text.push(self.advance().unwrap());
                    radix = 2;
                }
                _ => {}
            }
        }

        loop {
            match self.current() {
                Some('_') => { self.advance(); }
                Some(c) if c.is_digit(radix) => { text.push(c); self.advance(); }
                _ => break,
            }
        }

        if (radix == 10 || radix == 16) &&
           self.current() == Some('.') &&
           self.peek().map_or(false, |c| c.is_digit(radix))
        {
            is_float = true;
            text.push('.');
            self.advance();
            loop {
                match self.current() {
                    Some('_') => { self.advance(); }
                    Some(c) if c.is_digit(radix) => { text.push(c); self.advance(); }
                    _ => break,
                }
            }
        }

        match self.current() {
            Some('e') | Some('E') if radix == 10 => {
                is_float = true;
                text.push(self.advance().unwrap());
                if let Some(s @ ('+' | '-')) = self.current() { text.push(s); self.advance(); }
                while self.current().map_or(false, |c| c.is_ascii_digit()) {
                    text.push(self.advance().unwrap());
                }
            }
            Some('p') | Some('P') if radix == 16 => {
                is_float = true;
                text.push(self.advance().unwrap());
                if let Some(s @ ('+' | '-')) = self.current() { text.push(s); self.advance(); }
                while self.current().map_or(false, |c| c.is_ascii_digit()) {
                    text.push(self.advance().unwrap());
                }
            }
            _ => {}
        }

        if self.current() == Some('i') { is_imag = true; self.advance(); }

        let span = self.span_from(lo);
        if is_imag {
            let v: f64 = text.parse().map_err(|_| LexError {
                msg: format!("angka imajiner `{}i` tidak sah", text), span })?;
            return Ok(Token::new(TokenKind::ImaginaryLit(v), span));
        }
        if is_float {
            let v: f64 = text.parse().map_err(|_| LexError {
                msg: format!("angka pecahan `{}` tidak sah", text), span })?;
            return Ok(Token::new(TokenKind::FloatLit(v), span));
        }
        let v = u128::from_str_radix(
            text.trim_start_matches("0x").trim_start_matches("0o")
                .trim_start_matches("0b"),
            radix
        ).map_err(|_| LexError {
            msg: format!("angka bulat `{}` tidak sah", text), span })?;
        Ok(Token::new(TokenKind::IntLit(v), span))
    }

    // ── Main tokenization loop ────────────────────────────────────────────

    pub(crate) fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::with_capacity(self.chars.len() / 4);
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            // `let t = d.type` di akhir baris: kata kunci sesudah `::`/`.` adalah NAMA, dan
            // tanpa ini baris berikutnya ditelan sebagai argumen panggilan tanpa kurung.
            let jadi_nama = is_keyword_token(&tok.kind)
                && matches!(tokens.last().map(|t: &Token| &t.kind),
                            Some(TokenKind::ColonColon | TokenKind::Dot));
            self.last_token_auto_semi = triggers_auto_semi(&tok.kind) || jadi_nama;
            tokens.push(tok);
            if is_eof { break; }
        }
        Ok(tokens)
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn lex_src(src: &str) -> Vec<TokenKind> {
        let sf = SourceFile { id: 0, name: "test".to_string(), src: src.to_string() };
        lex(&sf).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    pub(crate) fn test_basic_let() {
        let kinds = lex_src("let x = 42");
        assert_eq!(kinds, vec![
            TokenKind::KwLet,
            TokenKind::Ident("x".to_string()),
            TokenKind::Eq,
            TokenKind::IntLit(42),
            TokenKind::Eof,
        ]);
    }

    #[test]
    pub(crate) fn test_hex_literal() {
        let kinds = lex_src("0xFF_00");
        assert!(matches!(kinds[0], TokenKind::IntLit(0xFF00)));
    }

    #[test]
    pub(crate) fn test_binary_literal() {
        let kinds = lex_src("0b1010_1010");
        assert!(matches!(kinds[0], TokenKind::IntLit(0b10101010)));
    }

    #[test]
    pub(crate) fn test_float() {
        let kinds = lex_src("3.14e-2");
        assert!(matches!(kinds[0], TokenKind::FloatLit(_)));
    }

    #[test]
    pub(crate) fn test_string_escape() {
        let kinds = lex_src(r#""hello\nworld""#);
        assert!(matches!(&kinds[0], TokenKind::StringLit(s) if s == "hello\nworld"));
    }

    #[test]
    pub(crate) fn test_spawn_keyword() {
        let kinds = lex_src("spawn { go compute() }");
        assert_eq!(kinds[0], TokenKind::KwSpawn);
        assert_eq!(kinds[1], TokenKind::LBrace);
        assert_eq!(kinds[2], TokenKind::KwGo);
    }

    #[test]
    pub(crate) fn test_auto_semicolon() {
        let kinds = lex_src("return x\nlet y = 1");
        // After `x` followed by newline, semicolon should be inserted
        assert!(kinds.contains(&TokenKind::Semi));
    }

    #[test]
    pub(crate) fn test_channel_ops() {
        let kinds = lex_src("ch <- val");
        assert_eq!(kinds[1], TokenKind::LArrow);
    }

    #[test]
    pub(crate) fn test_pipe_operator() {
        let kinds = lex_src("data |> transform");
        assert_eq!(kinds[1], TokenKind::Pipe);
    }

    #[test]
    pub(crate) fn test_nested_block_comment() {
        let kinds = lex_src("/* outer /* inner */ still outer */ x");
        assert!(matches!(&kinds[0], TokenKind::Ident(s) if s == "x"));
    }

    #[test]
    pub(crate) fn test_gc_ownership_annotation() {
        let kinds = lex_src("let @gc conn = dial(addr)");
        assert_eq!(kinds[0], TokenKind::KwLet);
        assert_eq!(kinds[1], TokenKind::At);
        assert_eq!(kinds[2], TokenKind::KwGc);
    }

    /// Rune satu huruf pernah rusak total: `'a'` terbaca sebagai label
    /// lifetime `'a`, menyisakan kutip penutup yang membuka rune baru.
    /// Hanya rune non-huruf (`'1'`, `' '`) yang kebetulan lolos.
    #[test]
    pub(crate) fn test_rune_literal_single_letter() {
        assert_eq!(lex_src("'a'")[0], TokenKind::RuneLit('a'));
        assert_eq!(lex_src("'Z'")[0], TokenKind::RuneLit('Z'));
        assert_eq!(lex_src("'_'")[0], TokenKind::RuneLit('_'));
        // yang dulu sudah jalan harus tetap jalan
        assert_eq!(lex_src("'1'")[0], TokenKind::RuneLit('1'));
        assert_eq!(lex_src("' '")[0], TokenKind::RuneLit(' '));
        assert_eq!(lex_src("'\\n'")[0], TokenKind::RuneLit('\n'));
        // non-ASCII
        assert_eq!(lex_src("'é'")[0], TokenKind::RuneLit('é'));
    }

    /// Label lifetime tetap dikenali — pembedanya adalah TIDAK ada kutip
    /// penutup tepat setelah satu karakter.
    #[test]
    pub(crate) fn test_lifetime_label_still_lexes() {
        assert_eq!(lex_src("'outer")[0], TokenKind::Ident("'outer".to_string()));
        assert_eq!(lex_src("'a")[0], TokenKind::Ident("'a".to_string()));
    }

    #[test]
    pub(crate) fn kata_kunci_sebagai_nama_memicu_titik_koma() {
        // Dulu `let t = d.type` di akhir baris menelan baris berikutnya sebagai argumen.
        for (src, kw) in [("let f = wokwok::var\nwok(1)", TokenKind::KwVar),
                          ("let t = d.type\nwok(t)", TokenKind::KwType),
                          ("let r = term::raw\nr()", TokenKind::KwRaw)] {
            let k = lex_src(src);
            let i = k.iter().position(|t| *t == kw).expect("kata kuncinya ada");
            assert_eq!(k[i + 1], TokenKind::Semi, "{:?}: {:?}", src, k);
        }
        // Kata kunci di tempatnya sendiri tetap menyambung baris: `x as` lalu `int`.
        let k = lex_src("let a = x as\nint");
        let i = k.iter().position(|t| *t == TokenKind::KwAs).expect("`as` ada");
        assert_ne!(k[i + 1], TokenKind::Semi, "{:?}", k);
    }

    #[test]
    pub(crate) fn tiap_kata_kunci_dikenali_sebagai_kata_kunci() {
        // Yang terlewat di `is_keyword_token` tidak bisa jadi nama sesudah `.`/`::`, dan
        // di akhir baris menelan baris berikutnya.
        let mut n = 0;
        for baris in include_str!("token.rs").lines() {
            let Some(sisa) = baris.trim().strip_prefix('"') else { continue };
            let Some((kata, kanan)) = sisa.split_once('"') else { continue };
            if !kanan.contains("=> TokenKind::Kw") { continue; }
            assert!(is_keyword_token(&keyword_or_ident(kata)), "`{}` tidak dikenali", kata);
            n += 1;
        }
        assert!(n >= 45, "cuma {} kata kunci terbaca dari token.rs", n);
    }
}
