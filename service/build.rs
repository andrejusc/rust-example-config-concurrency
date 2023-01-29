use std::env;  
use copy_to_output::copy_to_output;
  
fn main() {  
    // Way for re-runs script if any files under env folder have changed  
    // println!("cargo:rerun-if-changed=env/*");
    // Having re-runs unconditional
    let output_dir = format!("{}/{}", env::var("TARGET").unwrap(), env::var("PROFILE").unwrap());
    copy_to_output("env", &output_dir).expect("Could not copy");
}
