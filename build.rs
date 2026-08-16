fn main() {
    println!("cargo:rerun-if-env-changed=RUN_DOG_UPDATE_REPOSITORY");
    println!("cargo:rerun-if-changed=assets/rundog.ico");
    println!("cargo:rerun-if-changed=assets/rundog.rc");

    if let Ok(repository) = std::env::var("RUN_DOG_UPDATE_REPOSITORY") {
        println!("cargo:rustc-env=RUN_DOG_UPDATE_REPOSITORY={repository}");
    }

    embed_resource::compile("assets/rundog.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("embed RunDog application icon");
}
