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

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
export TARGET_CC=x86_64-w64-mingw32-gcc
export TARGET_CXX=x86_64-w64-mingw32-g++
