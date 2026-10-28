fn main() {
    // 图标文件变更时必须重跑（否则资源不更新，exe/任务管理器仍是旧图标）
    println!("cargo:rerun-if-changed=assets/bosskey.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon("assets/bosskey.ico")
            .compile()
            .expect("winres failed");
    }
}
