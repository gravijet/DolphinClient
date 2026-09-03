# Source this to cross-compile the Windows .exe builds from Linux.
#   source deploy/win-cross-env.sh
#
# Requires (one-time, on this build host):
#   rustup target add x86_64-pc-windows-gnu                 # for stable (launcher)
#   rustup target add --toolchain nightly-2026-07-01 x86_64-pc-windows-gnu # client
#   sudo apt-get install -y gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64
#
# Rust GUI apps *can* be cross-compiled to windows-gnu with mingw — both the
# eframe launcher and the wgpu/azalea client build cleanly this way, so the
# Windows .exe can be produced on this Linux server without a Windows machine.

# Target-scoped (`<var>_<target-triple>`) — the `cc` crate only applies these
# to a build that actually targets x86_64-pc-windows-gnu, so it's safe to
# leave them exported for the rest of the shell session.
#
# Deliberately NOT setting the generic `TARGET_CC`/`TARGET_CXX` here: unlike
# the target-scoped vars above, `cc` applies those to *any* build in this
# shell regardless of its actual target (see cc-rs's own env-var priority
# list) — a later *native* Linux build in the same `release.sh` run would
# then try to compile its C deps (e.g. aws-lc-sys) with the mingw cross
# compiler and fail outright. Cost one release a hung retry loop.
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
