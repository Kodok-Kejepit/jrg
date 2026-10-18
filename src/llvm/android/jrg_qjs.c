// Penjalan QuickJS minimal untuk jrgm: `qjs berkas.js|.mjs [argumen...]`, tanpa REPL.
#include <stdio.h>
#include <string.h>
#include "quickjs-libc.h"

static JSContext *konteks_baru(JSRuntime *rt) {
    JSContext *ctx = JS_NewContext(rt);
    if (!ctx) return NULL;
    js_init_module_std(ctx, "std");
    js_init_module_os(ctx, "os");
    return ctx;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "pemakaian: qjs berkas.js [argumen...]\n"); return 2; }
    setvbuf(stdout, NULL, _IOLBF, 0);
    JSRuntime *rt = JS_NewRuntime();
    if (!rt) { fprintf(stderr, "qjs: tidak bisa membuat runtime\n"); return 2; }
    js_std_set_worker_new_context_func(konteks_baru);
    js_std_init_handlers(rt);
    JSContext *ctx = konteks_baru(rt);
    if (!ctx) { fprintf(stderr, "qjs: tidak bisa membuat konteks\n"); return 2; }
    JS_SetModuleLoaderFunc(rt, NULL, js_module_loader, NULL);
    JS_SetHostPromiseRejectionTracker(rt, js_std_promise_rejection_tracker, NULL);
    js_std_add_helpers(ctx, argc - 1, argv + 1);
    size_t n;
    uint8_t *isi = js_load_file(ctx, &n, argv[1]);
    if (!isi) { perror(argv[1]); return 1; }
    size_t p = strlen(argv[1]);
    int modul = (p > 4 && !strcmp(argv[1] + p - 4, ".mjs")) || JS_DetectModule((const char *)isi, n);
    int gagal = 0;
    JSValue v;
    if (modul) {
        v = JS_Eval(ctx, (const char *)isi, n, argv[1], JS_EVAL_TYPE_MODULE | JS_EVAL_FLAG_COMPILE_ONLY);
        if (!JS_IsException(v)) {
            js_module_set_import_meta(ctx, v, 1, 1);
            v = JS_EvalFunction(ctx, v);
        }
    } else {
        v = JS_Eval(ctx, (const char *)isi, n, argv[1], JS_EVAL_TYPE_GLOBAL);
    }
    if (JS_IsException(v)) { js_std_dump_error(ctx); gagal = 1; }
    JS_FreeValue(ctx, v);
    js_free(ctx, isi);
    if (!gagal) js_std_loop(ctx);
    js_std_free_handlers(rt);
    JS_FreeContext(ctx);
    JS_FreeRuntime(rt);
    return gagal;
}
