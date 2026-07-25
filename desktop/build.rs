fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if let Err(failure) = embed_resource::compile("app.rc", embed_resource::NONE).manifest_optional()
    {
        panic!("the studio icon could not be compiled into the shell: {failure:?}");
    }
}
