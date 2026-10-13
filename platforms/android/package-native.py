#!/usr/bin/env python3
"""Builds the Rust editor for Android and its Kotlin bindings.

    platforms/android/package-native.py [--debug] [--arch arm64|x86_64]

Writes into platforms/android/rs-editor/generated/:
  kotlin/   the BoltFFI Kotlin class (NativeEditor) and its JNI glue
  jniLibs/  librs_editor_android.so per ABI

Same approach as rs-organizer's tools/package-android.py: BoltFFI 0.29.3's
`pack` links with whole-archive, which duplicates the HarfBuzz objects in
Skia's archives. Linking the staticlib normally keeps only what the JNI glue
and our own JNI functions (shell.rs) reference, and the C++ driver puts
libc++ in the .so.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess

TARGETS = {
    "arm64": ("aarch64-linux-android", "arm64-v8a"),
    "x86_64": ("x86_64-linux-android", "x86_64"),
}


def run(*args, cwd=None, env=None, capture=False):
    result = subprocess.run([str(a) for a in args], cwd=cwd, env=env,
                            stderr=subprocess.PIPE if capture else None, text=True)
    if result.returncode != 0:
        raise SystemExit(f"{result.stderr or ''}failed: {' '.join(str(a) for a in args)}")
    return result.stderr if capture else None


def android_config(path):
    """The [targets.android*] values of boltffi.toml (macOS ships Python
    3.9, without tomllib)."""
    sections, current = {}, None
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if header := re.fullmatch(r"\[(.+)\]", line):
            current = sections.setdefault(header.group(1), {})
        elif "=" in line and current is not None:
            key, value = (part.strip() for part in line.split("=", 1))
            current[key] = eval(value.replace("true", "True").replace("false", "False"))
    return sections


def ndk_bin():
    sdk = Path(os.environ.get("ANDROID_HOME", Path.home() / "Library/Android/sdk"))
    ndk = os.environ.get("ANDROID_NDK_HOME")
    if not ndk:
        versions = sorted((sdk / "ndk").iterdir(), key=lambda p: [int(x) for x in p.name.split(".")])
        ndk = versions[-1]
    hosts = list((Path(ndk) / "toolchains/llvm/prebuilt").glob("*/bin"))
    if len(hosts) != 1:
        raise SystemExit("Expected one NDK host toolchain")
    return Path(ndk), hosts[0]


def main():
    parser = argparse.ArgumentParser()
    # Release by default: an unoptimized editor is too slow to judge on a device.
    parser.add_argument("--debug", action="store_true")
    parser.add_argument("--arch", choices=list(TARGETS), help="one ABI only (default: all in boltffi.toml)")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[2]
    crate = root / "crates/rs-editor-android"
    config = android_config(crate / "boltffi.toml")
    android = config["targets.android"]
    kotlin = (crate / config["targets.android.kotlin"]["output"]).resolve()
    libs = (crate / config["targets.android.pack"]["output"]).resolve()
    api = android["min_sdk"]
    ndk, bin_dir = ndk_bin()
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", root / "target"))
    profile = "debug" if args.debug else "release"
    build_args = [] if args.debug else ["--release"]

    # Bindings come from metadata that BoltFFI's macros embed when the
    # crate is compiled for the host with special environment variables.
    # Cargo does not track those, so use a target directory of its own.
    generate_env = os.environ.copy()
    generate_env["CARGO_TARGET_DIR"] = str(target_dir / "boltffi-metadata")
    run("boltffi", "generate", "kotlin", cwd=crate, env=generate_env)

    for arch in [args.arch] if args.arch else android["architectures"]:
        triple, abi = TARGETS[arch]
        clang = bin_dir / f"{triple}{api}-clang"
        cpp = bin_dir / f"{triple}{api}-clang++"
        under = triple.replace("-", "_")
        # The native wrappers BoltFFI's JNI glue calls are only emitted by a
        # binding-expansion build.
        env = os.environ.copy()
        env.update({
            "ANDROID_NDK_HOME": str(ndk),
            "BOLTFFI_BINDING_EXPANSION": "1",
            "BOLTFFI_BINDING_EXPANSION_ROOT": str(crate),
            "BOLTFFI_BINDING_EXPANSION_SOURCE": str(crate / "src/lib.rs"),
            "BOLTFFI_BINDING_EXPANSION_SURFACE": "native",
            "BOLTFFI_BINDING_METADATA_FEATURES": "",
            f"CARGO_TARGET_{under.upper()}_LINKER": str(clang),
            f"CC_{under}": str(clang),
            f"CXX_{under}": str(cpp),
            f"AR_{under}": str(bin_dir / "llvm-ar"),
        })
        output = run("cargo", "rustc", "--lib", "--locked", "-p", "rs-editor-android", "--target", triple,
                     "--crate-type", "staticlib", *build_args,
                     "--", "--cfg", "boltffi_binding_expansion", "--print", "native-static-libs",
                     cwd=root, env=env, capture=True)
        print(output, end="")
        native_libs = next((line.split("native-static-libs:", 1)[1].split()
                            for line in output.splitlines() if "native-static-libs:" in line), [])
        archive = target_dir / triple / profile / "librs_editor_android.a"

        work = target_dir / "boltffi-android" / triple / profile
        work.mkdir(parents=True, exist_ok=True)
        glue = work / "jni_glue.o"
        exports = work / "exports.map"
        exports.write_text("{ global: Java_*; JNI_OnLoad*; JNI_OnUnload*; boltffi_*; local: *; };\n")
        run(clang, "-c", "-fPIC", "-ffunction-sections", "-fdata-sections", "-O0" if args.debug else "-O2",
            "-I", kotlin / "jni", kotlin / "jni/jni_glue.c", "-o", glue)
        # Nothing references our own JNI functions at link time; keep them.
        symbols = subprocess.run([bin_dir / "llvm-nm", "--defined-only", "-j", archive],
                                 check=True, capture_output=True, text=True).stdout.split()
        keep = sorted({s for s in symbols if s.startswith("Java_com_rseditor_android_")})
        destination = libs / abi / "librs_editor_android.so"
        destination.parent.mkdir(parents=True, exist_ok=True)
        run(cpp, "-shared", "-static-libstdc++", "-o", destination, glue,
            *[f"-Wl,--undefined={s}" for s in keep], archive,
            f"-Wl,--version-script={exports}", "-Wl,--gc-sections", "-Wl,--no-undefined",
            "-Wl,-z,nodelete", "-Wl,-z,max-page-size=16384",
            *[lib for lib in native_libs if lib not in ("-lc++", "-lc++_shared", "-lc++abi")],
            "-lEGL", "-landroid", "-llog", "-lm", "-ldl")
        run(bin_dir / "llvm-strip", "--strip-unneeded", destination)
        print(f"Packaged {destination} ({destination.stat().st_size // 1024} KB)", flush=True)


if __name__ == "__main__":
    main()
