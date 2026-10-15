fn main() {
    println!("cargo:rerun-if-changed=../../icons/windows/icon.ico");
    println!("cargo:rerun-if-changed=windows-service.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let resource = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("windows-resources.rc");
        tauri_winres::WindowsResource::new()
            .set_icon("../../icons/windows/icon.ico")
            .set_manifest_file("windows-service.manifest")
            .set("FileDescription", "MangoDisk Privileged Service")
            .set("ProductName", "MangoDisk")
            .set("OriginalFilename", "MangoDiskPrivileged.exe")
            .write_resource_file(&resource)
            .expect("failed to embed privileged service Windows resources");
        // Explicit privileges prevent installer heuristics from elevating setup tests.
        // The application requests elevation only for installation and removal.
        embed_resource::compile_for_everything(resource, embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile privileged service Windows resources");
    }
}
