fn main() {
    // 图标文件变更时必须重跑（否则资源不更新，exe/任务管理器仍是旧图标）
    println!("cargo:rerun-if-changed=assets/bosskey.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon("assets/bosskey.ico")
            // 版本号（FileVersion/ProductVersion）由 winres 读取 Cargo.toml 自动填充
            .set("ProductName", "BossKey")
            .set("FileDescription", "Windows 老板键")
            .set("CompanyName", "brucevon")
            .set("LegalCopyright", "Copyright (C) 2026 brucevon · GPL-3.0")
            .set("OriginalFilename", "BossKey.exe")
            .set("InternalName", "BossKey")
            .compile()
            .expect("winres failed");
    }
}
