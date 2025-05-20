use async_trait::async_trait;
use parity_scale_codec::Encode;
use serde_json::{json, Value};
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::marker::PhantomData;
use std::ops::Deref;
use subxt::{
    backend::legacy::LegacyRpcMethods,
    blocks::BlocksClient,
    config::{
        transaction_extensions,
        substrate::{BlakeTwo256, SubstrateHeader},
    },
    events::EventsClient,
    storage::StorageClient,
    tx::TxClient,
    Config, OnlineClient,
};

use subxt::Metadata;
use parity_scale_codec::Decode;
use frame_metadata::RuntimeMetadataPrefixed;

pub type BlockNumber = u64;
pub type RpcClientHeader = SubstrateHeader<BlockNumber, BlakeTwo256>;

#[derive(scale_encode::EncodeAsType, Clone)]
pub enum CustomConfig {}

impl Config for CustomConfig {
    type Hash = subxt::utils::H256;
    type AccountId = subxt::utils::AccountId32;
    type Address = subxt::utils::MultiAddress<Self::AccountId, u32>;
    type Signature = subxt::utils::MultiSignature;
    type Hasher = BlakeTwo256;
    type Header = SubstrateHeader<BlockNumber, Self::Hasher>;
    type ExtrinsicParams = transaction_extensions::AnyOf<
        Self,
        (
            transaction_extensions::CheckSpecVersion,
            transaction_extensions::CheckTxVersion,
            transaction_extensions::CheckNonce,
            transaction_extensions::CheckGenesis<Self>,
            transaction_extensions::CheckMortality<Self>,
            transaction_extensions::ChargeAssetTxPayment<Self>,
            transaction_extensions::ChargeTransactionPayment,
            transaction_extensions::CheckMetadataHash,
        ),
    >;
    type AssetId = u32;
}

#[async_trait]
pub trait SubstrateRpcClient {
    async fn get_raw_metadata(&mut self, block_num: Option<u64>) -> Result<Vec<u8>, String>;
}

pub struct SubxtClient<ChainConfig: Config> {
    legacy: LegacyRpcMethods<ChainConfig>,
    events: EventsClient<ChainConfig, OnlineClient<ChainConfig>>,
    tx: TxClient<ChainConfig, OnlineClient<ChainConfig>>,
    storage: StorageClient<ChainConfig, OnlineClient<ChainConfig>>,
    blocks: BlocksClient<ChainConfig, OnlineClient<ChainConfig>>,
}

#[async_trait]
impl<ChainConfig: Config<AccountId = subxt::utils::AccountId32, Header = RpcClientHeader>>
    SubstrateRpcClient for SubxtClient<ChainConfig>
{
    async fn get_raw_metadata(&mut self, block_num: Option<u64>) -> Result<Vec<u8>, String> {
        let maybe_hash = self
            .legacy
            .chain_get_block_hash(block_num.map(|b| b.into()))
            .await
            .map_err(|e| format!("Failed to get block hash: {:?}", e))?;
        println!("maybe_hash: {:?}", maybe_hash);

        let metadata = self
            .legacy
            .state_get_metadata(maybe_hash)
            .await
            .map_err(|e| format!("Failed to fetch metadata: {:?}", e))?;
        Ok(metadata.into_raw())
    }
}

#[derive(Clone)]
pub struct SubxtClientFactory<ChainConfig: Config> {
    url: String,
    _phantom: PhantomData<ChainConfig>,
}

impl<ChainConfig: Config<AccountId = subxt::utils::AccountId32, Header = RpcClientHeader>>
    SubxtClientFactory<ChainConfig>
{
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
pub trait SubstrateRpcClientFactory<RpcClient: SubstrateRpcClient> {
    async fn new_client(&self) -> Result<RpcClient, String>;
}

#[async_trait]
impl<ChainConfig: Config<AccountId = subxt::utils::AccountId32, Header = RpcClientHeader>>
    SubstrateRpcClientFactory<SubxtClient<ChainConfig>> for SubxtClientFactory<ChainConfig>
{
    async fn new_client(&self) -> Result<SubxtClient<ChainConfig>, String> {
        let rpc_client = subxt::backend::rpc::reconnecting_rpc_client::RpcClient::builder()
            .build(self.url.clone())
            .await
            .map_err(|e| format!("Could not create RpcClient: {:?}", e))?;
        let legacy = LegacyRpcMethods::new(rpc_client.into());
        let online_client = OnlineClient::from_insecure_url(self.url.clone())
            .await
            .map_err(|e| format!("Could not create OnlineClient: {:?}", e))?;
        let events = online_client.events();
        let tx = online_client.tx();
        let storage = online_client.storage();
        let blocks = online_client.blocks();
        Ok(SubxtClient {
            legacy,
            events,
            tx,
            storage,
            blocks,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // let factory = SubxtClientFactory::<CustomConfig>::new("ws://localhost:9944");
    // let factory = SubxtClientFactory::<CustomConfig>::new("wss://polkadot-rpc.publicnode.com");
    // let factory = SubxtClientFactory::<CustomConfig>::new("wss://rpc.heima-parachain.heima.network");
    let factory = SubxtClientFactory::<CustomConfig>::new("wss://rpc.paseo-parachain.heima.network");
    let mut client = factory.new_client().await?;
    let metadata_result = client.get_raw_metadata(None).await;
    let response: Value = match metadata_result {
        Ok(metadata_bytes) => {
            let runtime_metadata =
                    RuntimeMetadataPrefixed::decode(&mut metadata_bytes.as_slice()).unwrap();
            // println!("runtime_metadata: {:?}", runtime_metadata);
            let metadata_struct = Metadata::try_from(runtime_metadata).unwrap();
            // println!("metadata_struct: {:?}", metadata_struct);

            let metadata_hex = format!("0x{}", hex::encode(&metadata_bytes));
            json!({
                "jsonrpc": "2.0",
                "result": metadata_hex,
                "id": 1
            })
        }
        Err(e) => {
            json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32603,
                    "message": e
                },
                "id": 1
            })
        }
    };

    let mut file = File::create("metadata.json")?;
    file.write_all(serde_json::to_string_pretty(&response)?.as_bytes())?;
    file.flush()?;

    println!("Metadata written to metadata.json");

    Ok(())
}
