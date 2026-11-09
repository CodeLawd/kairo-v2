fn main() {
    println!("{:?}", grafton_ndi::NDI::new().map(|_| "initialised"));
}
