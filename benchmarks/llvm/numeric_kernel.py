#!/usr/bin/env python3
import sys

def kernel(iterations: int) -> int:
    x = 305419896
    for _ in range(iterations):
        x = ((x * 1664525 + 1013904223) & 0xFFFFFFFFFFFFFFFF) ^ (x >> 13)
    return x

if __name__ == "__main__":
    print(kernel(200000000))
