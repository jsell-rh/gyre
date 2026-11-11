"""One memory-bounded build environment shared by agents and verification."""

def environment():
    return {
        'CARGO_BUILD_JOBS':'1','RUST_TEST_THREADS':'1','CARGO_INCREMENTAL':'0',
        'CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0',
        'CARGO_PROFILE_DEV_INCREMENTAL':'false','CARGO_PROFILE_TEST_INCREMENTAL':'false',
        'RUSTFLAGS':'-C link-arg=-fuse-ld=mold -C link-arg=-Wl,--threads=1',
        'MALLOC_ARENA_MAX':'2','NODE_OPTIONS':'--max-old-space-size=2048',
        'CARGO_TARGET_DIR':'/tmp/gyre-build/target','CARGO_HOME':'/tmp/gyre-build/cargo',
        'NPM_CONFIG_CACHE':'/tmp/gyre-build/npm','RUSTUP_HOME':'/usr/local/rustup',
    }
