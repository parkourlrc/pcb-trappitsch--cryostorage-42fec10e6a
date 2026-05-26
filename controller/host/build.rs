//! Build script that contains all the information to build the slint UI.

use std::io;

fn main() -> io::Result<()> {
    set_build_version();

    // Slint build
    let config = slint_build::CompilerConfiguration::new();
    slint_build::compile_with_config("ui/main.slint", config).expect("Slint build failed");

    Ok(())
}

fn set_build_version() {
    let build_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let build_info = format!("build time: {}", build_time);
    println!("cargo:rustc-env=BUILD_INFO={build_info}");
}
