use burn_import::onnx::{ModelGen, RecordType};
use std::env;
use std::path::PathBuf;

fn main() {
    // Get the OUT_DIR where cargo expects build script outputs
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let model_dir = out_dir.join("model");
    
    // Generate the model code from the ONNX file.
    ModelGen::new()
        .input("src/model/mnist_fp32.onnx")
        .out_dir(model_dir.to_str().unwrap())  // Convert PathBuf to &str
        .record_type(RecordType::Bincode)
        .embed_states(true)
        .run_from_script();
    
    println!("cargo:rerun-if-changed=src/model/mnist_fp32.onnx");
}