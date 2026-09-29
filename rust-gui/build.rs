fn main() {
    slint_build::compile_with_config(
        "ui/app-window.slint",
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("failed to compile Phase 21 Slint application shell");
}
