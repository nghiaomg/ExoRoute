@echo off
setlocal

set "PROJECT_DIR=%~dp0"
cd /d "%PROJECT_DIR%"
if errorlevel 1 exit /b 1

echo [1/4] Building ExoRoute dashboard...
pushd web
if errorlevel 1 exit /b 1
call npm run build
if errorlevel 1 (
    popd
    exit /b 1
)
popd

echo [2/4] Building ExoRoute Windows release binary...
set "WINDOWS_TARGET_DIR=target"
tasklist /fi "imagename eq exoroute.exe" /fo csv /nh | find /i "exoroute.exe" >nul
if not errorlevel 1 (
    set "WINDOWS_TARGET_DIR=target\windows-build"
    echo An ExoRoute process is using the default executable. Building into target\windows-build instead.
)
cargo build --target-dir "%WINDOWS_TARGET_DIR%" --locked --release
if errorlevel 1 exit /b 1

where wsl.exe >nul 2>nul
if errorlevel 1 (
    echo WSL is required to build the Linux binary. Install and initialize WSL, then run this script again.
    exit /b 1
)

echo [3/4] Building ExoRoute Linux x86_64 release binary in WSL...
wsl.exe --cd "%PROJECT_DIR%" --exec sh -lc "if command -v cargo; then build_dir=/tmp/exoroute-linux-source; rm -rf $build_dir; mkdir -p $build_dir/data $build_dir/web/dist /tmp/exoroute-linux-target || exit 1; trap 'rm -rf $build_dir' EXIT; cp Cargo.toml Cargo.lock build.rs .env.example $build_dir/ && cp -a src $build_dir/src && cp data/provider-presets.yaml $build_dir/data/ && cp -a web/dist/. $build_dir/web/dist/ && cargo build --locked --release --target x86_64-unknown-linux-gnu --manifest-path $build_dir/Cargo.toml --target-dir /tmp/exoroute-linux-target && mkdir -p target/x86_64-unknown-linux-gnu/release && cp /tmp/exoroute-linux-target/x86_64-unknown-linux-gnu/release/exoroute target/x86_64-unknown-linux-gnu/release/exoroute.new-$$ && mv -f target/x86_64-unknown-linux-gnu/release/exoroute.new-$$ target/x86_64-unknown-linux-gnu/release/exoroute && ./target/x86_64-unknown-linux-gnu/release/exoroute --version; else echo 'Rust toolchain is missing in WSL. Install Rust with rustup in the default WSL distribution.'; exit 127; fi"
if errorlevel 1 (
    echo Linux build failed. The Windows binary is still available at %WINDOWS_TARGET_DIR%\release\exoroute.exe.
    exit /b 1
)

echo [4/4] Verifying Windows release binary...
"%WINDOWS_TARGET_DIR%\release\exoroute.exe" --version
if errorlevel 1 exit /b 1

echo.
echo Windows release binary: %PROJECT_DIR%%WINDOWS_TARGET_DIR%\release\exoroute.exe
echo Linux release binary:   %PROJECT_DIR%target\x86_64-unknown-linux-gnu\release\exoroute
endlocal
