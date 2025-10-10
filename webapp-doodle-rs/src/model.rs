// file: model.rs
// desc: Code to run model inference

use burn::{
    backend::ndarray::NdArray,
    tensor::Tensor,
};

use model::mnist::Model;

pub fn get_inference() -> u8 {

    type Backend = NdArray<f32>;

    // Get a default device for the backend
    let device = <Backend as burn::tensor::backend::Backend>::Device::default();

    // Create a new model and load the state
    let model: Model<Backend> = Model::default();

    // Run the model on the input
    let output = model.forward(input);

    // Get the index of the maximum value and return
    let digit_inference = output.argmax(1).into_scalar() as u8;
    digit_inference
}