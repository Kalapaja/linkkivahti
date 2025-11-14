use rand::prelude::*;

fn main() {
    let access_token: String = rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(30)
        .map(char::from)
        .collect();
    println!("cargo:rustc-env=ACCESS_TOKEN={}", access_token);
    println!("cargo:warning=Generated ACCESS_TOKEN: {}", access_token);
}
