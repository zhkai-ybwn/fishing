fn main() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/BossSensor.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/BossSensor.ico")
            .set("ProductName", "BossSensor")
            .set("FileDescription", "BossSensor desktop camera monitor")
            .compile()?;
    }
    Ok(())
}
