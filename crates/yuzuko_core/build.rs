fn main() {
  let target = std::env::var("TARGET").expect("TARGET");
  println!("cargo:rustc-env=YUZUKO_TARGET={target}");
  println!("cargo:rerun-if-changed=build.rs");
}
