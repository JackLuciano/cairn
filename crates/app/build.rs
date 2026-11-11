fn main() {
    println!("cargo:rerun-if-changed=assets/cairn.ico");
    println!("cargo:rerun-if-changed=locales");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/cairn.ico")
            .set("ProductName", "Cairn")
            .set("FileDescription", "Cairn")
            .set("CompanyName", "The Cairn contributors")
            .set("LegalCopyright", "MIT OR Apache-2.0");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
