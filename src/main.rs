use std::fs;

use axum::{Json, Router, extract::Request, http::StatusCode, response::IntoResponse, routing::{get, post}};
use axum_extra::extract::Multipart;
use serde_json::json;
use zip::{ZipWriter, unstable::LittleEndianWriteExt, write::FileOptions};

enum ApiError {
    NotFound,
    BadRequest(String),
    InternalError(String)
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "No resources could be found.".to_string()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, format!("There is something wrong with your request: {}", msg)),
            ApiError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Something went wrong. Probably not your fault: {}", msg)),
        };

        let body = Json(json!({
            "error": error_message
        }));

        (status, body).into_response()
    }
}

async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "message": "Server is running :)"
    }))
}

async fn download_log() -> Result<impl IntoResponse, ApiError> {
    use std::io::Write;
    use axum::response::Response;
    use axum::body::Body;
    use axum::http::header;
    
    let data_dir = "/opt/eotw_data";
    
    if !std::path::Path::new(data_dir).exists() {
        return Err(ApiError::NotFound);
    }
    let mut zip_buffer = Vec::new();
    {
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut zip_buffer));
        let options = FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);
        
        for entry in walkdir::WalkDir::new(data_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let name = path.strip_prefix(data_dir)
                .map_err(|e| ApiError::InternalError(format!("Path error: {}", e)))?;
            
            let file_data = fs::read(path)
                .map_err(|e| ApiError::InternalError(format!("Failed to read file: {}", e)))?;
            
            zip.start_file(name.to_string_lossy().to_string(), options)
                .map_err(|e| ApiError::InternalError(format!("Failed to add file to zip: {}", e)))?;
            
            zip.write(&file_data)
                .map_err(|e| ApiError::InternalError(format!("Failed to write to zip: {}", e)))?;
        }
        
        zip.finish()
            .map_err(|e| ApiError::InternalError(format!("Failed to finalize zip: {}", e)))?;
    }
    
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("logs_{}.zip", timestamp);
    
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename)
        )
        .body(Body::from(zip_buffer))
        .map_err(|e| ApiError::InternalError(format!("Failed to build response: {}", e)))?;
    
    Ok(response)
}

async fn upload_log(mut multipart: Multipart) -> Result<impl IntoResponse, ApiError> {
    let mut file_saved = false;

    // Create subfolder for each day
    let now = chrono::Local::now();
    let date_dir = now.format("%Y-%m-%d").to_string();
    let upload_dir = format!("/opt/eotw_data/{}", date_dir);
    fs::create_dir_all(&upload_dir)
        .map_err(|e| ApiError::InternalError(format!("Failed to create directory: {}", e)))?;
    
    // Iterate through file
    while let Some(field) = multipart.next_field().await
        .map_err(|e| ApiError::BadRequest(format!("Failed to read multipart field: {}", e)))? 
    {
        let name = field.name()
            .ok_or_else(|| ApiError::BadRequest("Field name is missing".to_string()))?
            .to_string();
        
        let file_name = field.file_name()
            .ok_or_else(|| ApiError::BadRequest("File name is missing".to_string()))?
            .to_string();
        
        let data = field.bytes().await
            .map_err(|e| ApiError::BadRequest(format!("Failed to read file data: {}", e)))?;
        
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let safe_file_name = format!("{}_{}", timestamp, file_name.replace(['/', '\\'], "_"));
        let file_path = format!("{}/{}", upload_dir, safe_file_name);
        
        fs::write(&file_path, &data)
            .map_err(|e| ApiError::InternalError(format!("Failed to save file: {}", e)))?;
        file_saved = true;
        println!("File uploaded: {} -> {}", file_name, file_path);
    }
    
    if !file_saved {
        return Err(ApiError::BadRequest("No file was uploaded".to_string()));
    }

    Ok(Json(json!({
        "status": "success",
        "message": "File uploaded successfully"
    })))
}

fn create_app() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/upload", post(upload_log))
        .route("/download", get(download_log))
}

#[tokio::main]
async fn main() {
    // Setup directory for data
    fs::create_dir_all("/opt/eotw_data").unwrap();

    // Serve app
    let app = create_app();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Failed to bind TCP Listener!");

    println!("Server running...");

    axum::serve(listener, app)
        .await
        .expect("failed to start server :(");
}
