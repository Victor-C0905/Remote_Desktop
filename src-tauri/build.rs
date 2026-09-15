fn main() {
    tauri_build::build();

    // Track icon assets so that `cargo tauri dev` / `cargo tauri build`
    // re-embed the logo when any icon file changes. Tauri v2's
    // tauri_build::build() only registers tauri.conf.json as a rebuild
    // dependency by default, so swapping icon bytes alone never triggered
    // a recompile (the old binary kept the previous icon).
    println!("cargo:rerun-if-changed=icons/32x32.png");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    println!("cargo:rerun-if-changed=icons/128x128@2x.png");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.icns");
    println!("cargo:rerun-if-changed=icons/Square30x30Logo.png");
    println!("cargo:rerun-if-changed=icons/Square44x44Logo.png");
    println!("cargo:rerun-if-changed=icons/Square71x71Logo.png");
    println!("cargo:rerun-if-changed=icons/Square89x89Logo.png");
    println!("cargo:rerun-if-changed=icons/Square107x107Logo.png");
    println!("cargo:rerun-if-changed=icons/Square142x142Logo.png");
    println!("cargo:rerun-if-changed=icons/Square150x150Logo.png");
    println!("cargo:rerun-if-changed=icons/Square284x284Logo.png");
    println!("cargo:rerun-if-changed=icons/Square310x310Logo.png");
    println!("cargo:rerun-if-changed=icons/StoreLogo.png");
}
