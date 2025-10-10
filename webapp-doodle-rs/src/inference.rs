// file: inference.rs
// desc: Code to run model inference

use burn::{
    backend::ndarray::NdArray,
    tensor::Tensor,
};

mod model;
use model::mnist::Model;

type Backend = NdArray<f32>;

// Convert 48x48 boolean grid to 28x28 float array for MNIST
fn downsample_canvas(canvas: &[[bool; 48]; 48]) -> [[f32; 28]; 28] {
    let mut result = [[0.0f32; 28]; 28];
    
    // Simple binning: each 28x28 output pixel averages a ~1.7x1.7 region
    // We'll use nearest neighbor sampling for simplicity
    for y in 0..28 {
        for x in 0..28 {
            // Map to source coordinates
            let src_x = ((x as f32 * 48.0) / 28.0) as usize;
            let src_y = ((y as f32 * 48.0) / 28.0) as usize;
            
            // Sample 2x2 region and average
            let mut count = 0;
            let mut sum = 0.0;
            
            for dy in 0..2 {
                for dx in 0..2 {
                    let sx = (src_x + dx).min(47);
                    let sy = (src_y + dy).min(47);
                    if canvas[sy][sx] {
                        sum += 1.0;
                    }
                    count += 1;
                }
            }
            
            result[y][x] = sum / count as f32;
        }
    }
    
    result
}

pub fn get_inference(canvas: &[[bool; 48]; 48]) -> u8 {
    // Check if canvas has any pixels drawn
    let has_pixels = canvas.iter().any(|row| row.iter().any(|&p| p));
    if !has_pixels {
        return 255; // Special value for "no drawing"
    }

    // Get a default device for the backend
    let device = <Backend as burn::tensor::backend::Backend>::Device::default();

    // Create a new model and load the state
    let model: Model<Backend> = Model::default();

    // Convert canvas to 28x28
    let downsampled = downsample_canvas(canvas);
    
    // Flatten to 1D array
    let mut input_data = Vec::with_capacity(28 * 28);
    for row in downsampled.iter() {
        for &pixel in row.iter() {
            input_data.push(pixel);
        }
    }

    // Create input tensor with shape [1, 1, 28, 28]
    // MNIST expects batch_size=1, channels=1, height=28, width=28
    let input = Tensor::<Backend, 1>::from_floats(input_data.as_slice(), &device)
        .reshape([1, 1, 28, 28]);

    // Run the model on the input
    let output = model.forward(input);

    // Get the index of the maximum value and return
    let digit_inference = output.argmax(1).into_scalar() as u8;
    digit_inference
}