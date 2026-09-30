fn main() {
    println!("cargo:rerun-if-changed=assets/stream-archive-icon.png");
    println!("cargo:rerun-if-changed=assets/stream-archive.ico");

    slint_build::compile_with_config(
        "ui/app-window.slint",
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile Slint application shell");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/stream-archive.ico")
            .set("ProductName", "Stream Archive")
            .set("FileDescription", "Stream Archive")
            .set("OriginalFilename", "StreamArchive.exe");
        resource
            .compile()
            .expect("failed to compile Windows application resources");
    }
}
