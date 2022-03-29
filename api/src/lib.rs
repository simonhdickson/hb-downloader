use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    convert::TryInto,
    path::{Path, PathBuf},
};

use futures_util::StreamExt;
use md5::Md5;
use reqwest::header::{HeaderMap, HeaderValue};
use sha1::{self, Digest, Sha1};
use thiserror::Error;
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
};
use url::Url;

mod types;

use types::{Download, DownloadStruct, Order, OrderListItem};

const BASE_URL: &str = "https://www.humblebundle.com/api/v1";

pub struct HBClient {
    client: reqwest::Client,
    headers: HeaderMap<HeaderValue>,
    download_folder: PathBuf,
    platforms: HashSet<String>,
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("error talking to humble bundle api")]
    Api(#[from] reqwest::Error),
    #[error("io error")]
    IO(#[from] std::io::Error),
    #[error("url parse error")]
    UrlParse(#[from] url::ParseError),
}

impl HBClient {
    pub fn new(
        download_folder: PathBuf,
        headers: HashMap<String, String>,
        platforms: HashSet<String>,
    ) -> Self {
        let headers: HeaderMap = (&headers).try_into().unwrap();
        Self {
            client: reqwest::Client::new(),
            download_folder,
            headers,
            platforms,
        }
    }

    pub async fn list_orders(&self) -> Result<Vec<OrderListItem>, ApiError> {
        let response = self
            .client
            .get(&*format!("{}/{}", BASE_URL, "user/order"))
            .headers(self.headers.clone())
            .send()
            .await?;

        let orders = response.json::<Vec<OrderListItem>>().await?;

        Ok(orders)
    }

    pub async fn get_order(&self, gamekey: &str) -> Result<Order, ApiError> {
        let response = self
            .client
            .get(&*format!("{}/{}/{}", BASE_URL, "order", gamekey))
            .headers(self.headers.clone())
            .send()
            .await?;

        let order = response.json::<Order>().await?;

        Ok(order)
    }

    pub async fn download_order(&self, order: &Order) -> Result<(), ApiError> {
        for product in &order.subproducts {
            for download in &product.downloads {
                self.download(download).await?;
            }
        }

        Ok(())
    }

    pub async fn download(&self, download: &Download) -> Result<(), ApiError> {
        for file in &download.download_struct {
            if file.url.is_none() {
                continue;
            }

            if !self.platforms.contains(&download.platform) {
                continue;
            }

            let download_url = Url::parse(&file.url.as_ref().unwrap().web)?;

            let (mut dest, file_name) = {
                let fname = download_url
                    .path_segments()
                    .and_then(|segments| segments.last())
                    .and_then(|name| if name.is_empty() { None } else { Some(name) })
                    .unwrap();

                let file_name = self.download_folder.join(fname);

                if file_name.exists() {
                    let mut input = File::open(&file_name).await?;
                    let mut content = Vec::new();
                    input.read_to_end(&mut content).await?;

                    if check_data_validity(file, file_name.as_path()).await? {
                        println!("valid {} already exists locally, ignoring", fname);
                        continue;
                    }
                }

                println!("downloading file {}", fname);

                (File::create(file_name.clone()).await?, file_name)
            };

            let response = self
                .client
                .get(download_url)
                .headers(self.headers.clone())
                .send()
                .await?;

            let total_size = response.content_length().unwrap_or(0);
            let mut downloaded: u64 = 0;
            let mut stream = response.bytes_stream();

            while let Some(item) = stream.next().await {
                let chunk = item?;
                dest.write_all(&chunk).await?;
                let new = min(downloaded + (chunk.len() as u64), total_size);
                downloaded = new;
            }

            //copy(&mut content, &mut dest).await?;

            drop(dest);

            if check_data_validity(file, file_name.as_path()).await? {}
        }

        Ok(())
    }
}

pub async fn check_data_validity(
    download_struct: &DownloadStruct,
    path: &Path,
) -> Result<bool, ApiError> {
    let file = File::open(path).await?;
    let reader = BufReader::new(file);

    if let Some(expected_hash) = &download_struct.sha1 {
        let file_hash = sha1_digest(reader).await?;

        if expected_hash != &file_hash {
            println!("expected sha1 {} got {}", expected_hash, file_hash);
            return Ok(false);
        }
    } else if let Some(expected_hash) = &download_struct.md5 {
        let file_hash = md5_digest(reader).await?;

        if expected_hash != &file_hash {
            println!("expected md5 {} got {}", expected_hash, file_hash);
            return Ok(false);
        }
    }

    Ok(true)
}

pub async fn sha1_digest<R: AsyncRead + Unpin>(mut reader: R) -> Result<String, ApiError> {
    let mut digest = Sha1::new();
    let mut buffer = [0; 1024];

    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }

    let hash = digest.finalize();

    Ok(format!("{:x}", hash))
}

pub async fn md5_digest<R: AsyncRead + Unpin>(mut reader: R) -> Result<String, ApiError> {
    let mut digest = Md5::new();
    let mut buffer = [0; 1024];

    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }

    let hash = digest.finalize();

    Ok(format!("{:x}", hash))
}
