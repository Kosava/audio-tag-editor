fn main() {
    // Fluent stil svuda: isti izgled na Linux-u i Windows-u
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )
    .expect("Slint build");

    // Ikonica u .exe — proverava CILJNI OS, pa radi i pri cross-compile sa Linux-a
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("data/audio-tag-editor.ico");
        res.compile().expect("Windows resource");
    }
}
