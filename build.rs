use cxx_qt_build::CxxQtBuilder;

fn main() {
    println!("cargo::rerun-if-changed=Main.qml");
    CxxQtBuilder::new()
        .files(["src/bridge.rs", "src/shim.rs"])
        .qrc_resources(["Main.qml"])
        .qt_module("Quick")
        .qt_module("Widgets")
        .qt_module("WebEngineCore")
        .qt_module("WebEngineQuick")
        .include_dir("cpp")
        .cpp_file("cpp/shim.cpp")
        .build();
    // the shim's static lib links after Qt; keep these past --as-needed
    for arg in [
        "-Wl,--push-state,--no-as-needed",
        "-lQt6Quick",
        "-lQt6Widgets",
        "-lQt6WebEngineCore",
        "-lQt6WebEngineQuick",
        "-Wl,--pop-state",
    ] {
        println!("cargo::rustc-link-arg={arg}");
    }
}
