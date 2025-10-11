// file: inference.rs
// desc: Code to run model inference

use burn::{
    backend::ndarray::NdArray,
    tensor::Tensor,
};

use crate::model::mnist::Model;

type Backend = NdArray<f32>;

fn find_bounding_box(canvas: &[[bool; 48]; 48]) -> Option<(usize, usize, usize, usize)> {
    let mut min_x = 48;
    let mut max_x = 0;
    let mut min_y = 48;
    let mut max_y = 0;
    let mut found = false;
    
    for (y, row) in canvas.iter().enumerate() {
        for (x, &pixel) in row.iter().enumerate() {
            if pixel {
                found = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    
    if found {
        Some((min_x, max_x, min_y, max_y))
    } else {
        None
    }
}

fn downsample_and_center(canvas: &[[bool; 48]; 48]) -> [[f32; 28]; 28] {
    let mut result = [[0.0f32; 28]; 28];
    
    let (min_x, max_x, min_y, max_y) = match find_bounding_box(canvas) {
        Some(bbox) => bbox,
        None => return result,
    };
    
    let width = max_x - min_x + 1;
    let height = max_y - min_y + 1;
    
    let target_size = 20;
    
    let scale = if width > height {
        target_size as f32 / width as f32
    } else {
        target_size as f32 / height as f32
    };
    
    let scaled_width = (width as f32 * scale) as usize;
    let scaled_height = (height as f32 * scale) as usize;
    
    let offset_x = (28 - scaled_width) / 2;
    let offset_y = (28 - scaled_height) / 2;
    
    for out_y in 0..scaled_height {
        for out_x in 0..scaled_width {
            let src_x = min_x + (out_x as f32 / scale) as usize;
            let src_y = min_y + (out_y as f32 / scale) as usize;
            
            let src_x_next = min_x + ((out_x + 1) as f32 / scale).ceil() as usize;
            let src_y_next = min_y + ((out_y + 1) as f32 / scale).ceil() as usize;
            
            let mut sum = 0.0;
            let mut count = 0;
            
            for sy in src_y..src_y_next.min(max_y + 1) {
                for sx in src_x..src_x_next.min(max_x + 1) {
                    if canvas[sy][sx] {
                        sum += 1.0;
                    }
                    count += 1;
                }
            }
            
            let value = if count > 0 { sum / count as f32 } else { 0.0 };
            result[offset_y + out_y][offset_x + out_x] = value;
        }
    }
    
    result
}

pub fn get_inference(canvas: &[[bool; 48]; 48]) -> u8 {
    let has_pixels = canvas.iter().any(|row| row.iter().any(|&p| p));
    if !has_pixels {
        return 255;
    }

    let device = <Backend as burn::tensor::backend::Backend>::Device::default();
    let model: Model<Backend> = Model::default();

    let processed = downsample_and_center(canvas);
    
    let mut input_data = Vec::with_capacity(28 * 28);
    for row in processed.iter() {
        for &pixel in row.iter() {
            input_data.push(pixel);
        }
    }

    let input = Tensor::<Backend, 1>::from_floats(input_data.as_slice(), &device)
        .reshape([1, 1, 28, 28]);

    let output = model.forward(input);
    let digit_inference = output.argmax(1).into_scalar() as u8;
    digit_inference
}