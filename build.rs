fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.ico");

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let res_file = format!("{}/app.o", out_dir);
        let status = Command::new("windres")
            .args(&["app.rc", "-O", "coff", "-o", &res_file])
            .status();
        if let Ok(s) = status {
            if s.success() {
                println!("cargo:rustc-link-arg={}", res_file);
            }
        }
    }
}
