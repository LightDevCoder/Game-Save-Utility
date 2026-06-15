fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");

    #[cfg(windows)]
    if std::path::Path::new("assets/app.ico").exists() {
        winresource::WindowsResource::new()
            .set_icon("assets/app.ico")
            .set("ProductName", "Game Save Utility")
            .set("FileDescription", "Game Save Utility")
            .compile()
            .expect("failed to compile Windows resources");
    }
}
