// Только для Windows: встраивает иконку в сам .exe через ресурсы PE.
//
// Без этого иконка видна только в заголовке окна, а в проводнике Windows
// файл показывается со стандартным значком приложения.

fn main() {
    println!("cargo:rerun-if-changed=assets/windows/glitch-studio.rc");
    println!("cargo:rerun-if-changed=assets/windows/glitch-studio.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let rc = std::path::Path::new("assets/windows/glitch-studio.rc");
    if !rc.exists() {
        return;
    }

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let obj = std::path::Path::new(&out_dir).join("glitch-studio-res.o");

    let status = std::process::Command::new(&windres)
        .args(["-i", "assets/windows/glitch-studio.rc", "-O", "coff", "-o"])
        .arg(&obj)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg={}", obj.display());
        }
        Ok(s) => println!("cargo:warning=windres завершился с кодом {s}, иконка не встроена"),
        Err(e) => println!("cargo:warning=не удалось запустить {windres}: {e}"),
    }
}
