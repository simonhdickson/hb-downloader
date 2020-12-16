use std::{
    collections::{HashMap, HashSet},
    convert::TryInto,
    path::PathBuf,
};

use async_std::{fs::File, io::copy, prelude::*};
use md5::Md5;
use reqwest::header::{HeaderMap, HeaderValue};
use sha1::{self, Digest, Sha1};
use thiserror::Error;
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
                break;
            }

            if !self.platforms.contains(&download.platform) {
                break;
            }

            let download_url = Url::parse(&file.url.as_ref().unwrap().web)?;

            let mut dest = {
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

                    if check_data_validity(file, &content) {
                        println!("valid {} already exists locally, ignoring", fname);
                        break;
                    }
                }

                println!("downloading file {}", fname);

                File::create(file_name).await?
            };

            let response = self
                .client
                .get(download_url)
                .headers(self.headers.clone())
                .send()
                .await?;

            let content = response.bytes().await?;
            let mut content = content.as_ref();

            if !check_data_validity(file, content) {
                continue;
            }

            copy(&mut content, &mut dest).await?;
            break;
        }

        Ok(())
    }
}

fn check_data_validity(download_struct: &DownloadStruct, content: &[u8]) -> bool {
    if let Some(expected_hash) = &download_struct.sha1 {
        let file_hash = format!("{:x}", Sha1::digest(&content));

        if expected_hash != &file_hash {
            println!("expected sha1 {} got {}", expected_hash, file_hash);
            return false;
        }
    } else if let Some(expected_hash) = &download_struct.md5 {
        let file_hash = format!("{:x}", Md5::digest(&content));

        if expected_hash != &file_hash {
            println!("expected md5 {} got {}", expected_hash, file_hash);
            return false;
        }
    }

    // yolo
    true
}
