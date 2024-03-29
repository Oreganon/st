use actix_multipart::Multipart;
use actix_web::{
    get, http::header::ContentType, post, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures_util::StreamExt as _;
use same_file::is_same_file;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;

const CONTENT_DIR: &str = "./content";
const RETAIN_SECS: u64 = 1800;
const MAX_SIZE_IN_BYTES: usize = 1_000_000;

#[get("/")]
async fn hello(req: HttpRequest) -> impl Responder {
    let host: &str = match req.headers().get("Host") {
        Some(a) => a.to_str().unwrap_or("??"),
        None => "st.pepog.com",
    };
    let response = format!(
        "{host} Tmp Hosting\n\n
curl -F'file=@yourfile.png' https://{host}"
    );
    HttpResponse::Ok().body(response)
}

#[get("/sgg")]
async fn up(_req: HttpRequest) -> impl Responder {
    let response = r#"
    <html>
    <body>
        <form action="/?direct=true" method="post" enctype="multipart/form-data">
            <input type="file" name="file" />
            <input type="submit" value="Upload" />
        </form>
    </body>
    "#;
    HttpResponse::Ok().body(response)
}

#[get("/v/{file}")]
async fn view(req: HttpRequest) -> impl Responder {
    let filename = req.match_info().get("file").unwrap();
    let supplied_path = format!("{CONTENT_DIR}/{filename}");

    let path = Path::new(&supplied_path);
    let parent = path.parent().unwrap();

    let content_dir = Path::new(CONTENT_DIR);

    if !is_same_file(content_dir, parent).unwrap() {
        return HttpResponse::Ok().body("no\n");
    }
    let content = fs::read_to_string(supplied_path).unwrap_or("Could not display".to_string());
    let response = format!("<html><body style=\"white-space: pre;\">{content}</body></html>");
    HttpResponse::Ok()
        .content_type(ContentType::html())
        .insert_header(("Content-Security-Policy", "script-src 'none'"))
        .body(response)
}

#[get("/{file}")]
async fn direct(req: HttpRequest) -> impl Responder {
    let filename = req.match_info().get("file").unwrap();
    let supplied_path = format!("{CONTENT_DIR}/{filename}");

    let path = Path::new(&supplied_path);
    let parent = path.parent().unwrap();

    let content_dir = Path::new(CONTENT_DIR);

    if !is_same_file(content_dir, parent).unwrap() {
        return HttpResponse::Ok().body("no\n");
    }

    let content = fs::read(supplied_path).unwrap();
    HttpResponse::Ok()
        .insert_header(("Content-Security-Policy", "script-src 'none'"))
        .body(content)
}

#[post("/")]
async fn upload(req: HttpRequest, mut payload: Multipart) -> impl Responder {
    let host: &str = match req.headers().get("Host") {
        Some(a) => a.to_str().unwrap_or("???"),
        None => "st.pepog.com",
    };

    let mut hasher = Sha256::new();
    let mut tempfile = NamedTempFile::new().unwrap();

    if let Some(length) = req.headers().get("content-length") {
        // PepoBan early but don't rely on user supplied content-length
        // The length is checked again below
        if length.to_str().unwrap().parse::<usize>().unwrap() > MAX_SIZE_IN_BYTES {
            return HttpResponse::Ok().body("too big\n");
        }
    };

    let item = payload.next().await.unwrap();
    let mut field = item.unwrap();

    let mut total_size = 0;
    // Field in turn is stream of *Bytes* object
    while let Some(chunk) = field.next().await {
        let chunk = &chunk.unwrap();
        total_size += chunk.len();
        if total_size > MAX_SIZE_IN_BYTES {
            return HttpResponse::Ok().body("too big\n");
        }
        hasher.update(chunk);
        tempfile.write(chunk).unwrap();
    }
    let user_provided_filename = field.content_disposition().get_filename();

    let result = hasher.finalize();
    let id = format!("{:x}", result).chars().take(6).collect::<String>();

    let (_file, path) = tempfile.keep().unwrap();

    let name = {
        let user_provided_ext = if let Some(name) = user_provided_filename {
            Path::new(name).extension().and_then(OsStr::to_str)
        } else {
            None
        };
        let guess = infer::get_from_path(path.clone());
        let ext_guess = if let Ok(Some(t)) = guess {
            Some(t.extension())
        } else {
            None
        };
        let ext = if user_provided_ext == None && ext_guess == None {
            return HttpResponse::Ok().body("cannot figure out file extension\n");
        } else if ext_guess != None {
            ext_guess.unwrap()
        } else {
            user_provided_ext.unwrap()
        };

        format!("{id}.{ext}")
    };

    let final_path = format!("{CONTENT_DIR}/{name}");
    fs::copy(path, final_path).unwrap();

    if req.query_string().contains("direct") {
        println!("Direct link: {host}/{name}");
        return HttpResponse::MovedPermanently()
            .insert_header(("Location", format!("/{name}")))
            .finish();
    }

    let response = format!("{host}/{name}\n{host}/v/{name}\n");
    HttpResponse::Ok().body(response)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    thread::spawn(|| {
        loop {
            for entry in fs::read_dir(CONTENT_DIR).unwrap() {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                if let Ok(time) = metadata.accessed() {
                    let now = std::time::SystemTime::now();
                    if let Ok(duration) = now.duration_since(time) {
                        if duration.as_secs() >= RETAIN_SECS {
                            fs::remove_file(entry.path()).unwrap();
                        }
                    } else {
                        // backwards time? PepoBan
                        fs::remove_file(entry.path()).unwrap();
                    }
                } else {
                    // No accessed time? PepoBan
                    fs::remove_file(entry.path()).unwrap();
                }
            }
            thread::sleep(Duration::from_secs(RETAIN_SECS.saturating_sub(3)));
        }
    });

    fs::create_dir_all(CONTENT_DIR).expect("Could not create \"host\" dir");
    HttpServer::new(|| {
        App::new()
            .service(hello)
            .service(up)
            .service(upload)
            .service(view)
            .service(direct)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
