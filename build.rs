fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Oxide");
        res.set("FileDescription", "Oxide Package Manager");
        res.compile().expect("Failed to compile Windows resources");
    }
}
