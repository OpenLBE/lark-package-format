fn main() {
    #[cfg(target_os = "windows")]
    {
        winresource::WindowsResource::new()
            .set_icon("logo.ico")
            .compile()
            .expect("failed to embed Windows resources");
    }
}
