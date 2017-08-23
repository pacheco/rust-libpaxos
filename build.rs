extern crate gcc;

fn main() {
    gcc::Build::new()
        .file("src/wrapper.c")
        .compile("wrapper");
}
