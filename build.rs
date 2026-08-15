fn main() {
    println!("cargo:rerun-if-env-changed=RUN_DOG_UPDATE_REPOSITORY");

    if let Ok(repository) = std::env::var("RUN_DOG_UPDATE_REPOSITORY") {
        println!("cargo:rustc-env=RUN_DOG_UPDATE_REPOSITORY={repository}");
    }
}
