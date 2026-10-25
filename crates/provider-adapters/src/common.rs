use std::collections::BTreeMap;
use serde::de::DeserializeOwned;
use stream_core::providers::{AdapterResult,CandidateStream,ErrorCategory,ProviderFailure,ProviderId};
use stream_provider_sdk::http::{Header,HostError,Request,Response};
use thiserror::Error;
use url::Url;

pub use stream_provider_sdk::http::ProviderHost;

pub const USER_AGENT:&str="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36";

#[derive(Debug,Error)]
pub enum AdapterError {
    #[error("invalid provider lookup")]
    Input,
    #[error("unsupported provider implementation")]
    Unsupported,
    #[error("provider returned no usable source: {0}")]
    Unavailable(&'static str),
    #[error("provider protocol changed: {0}")]
    Protocol(&'static str),
    #[error("provider host capability failed")]
    Host(#[from] HostError),
}
impl AdapterError {
    pub fn category(&self)->ErrorCategory {
        match self {
            Self::Input|Self::Unsupported=>ErrorCategory::Unsupported,
            Self::Unavailable(_)=>ErrorCategory::Unavailable,
            Self::Protocol(_)=>ErrorCategory::ProtocolChange,
            Self::Host(HostError::Cancelled)=>ErrorCategory::Cancelled,
            Self::Host(HostError::Timeout)=>ErrorCategory::Timeout,
            Self::Host(HostError::Denied|HostError::InvalidRequest)=>ErrorCategory::ProtocolChange,
            Self::Host(HostError::LimitExceeded)=>ErrorCategory::Unavailable,
            Self::Host(HostError::UpstreamFailure)=>ErrorCategory::UpstreamFailure,
        }
    }
}

pub fn json<T:DeserializeOwned>(bytes:&[u8],limit:usize,stage:&'static str)->Result<T,AdapterError>{
    if bytes.len()>limit{return Err(AdapterError::Protocol("response byte limit"));}
    serde_json::from_slice(bytes).map_err(|_|AdapterError::Protocol(stage))
}
pub fn header(name:&str,value:impl Into<String>)->Header{Header{name:name.into(),value:value.into()}}
pub fn get(url:impl Into<String>,referer:&str,limit:u32)->Request {
    Request{url:url.into(),method:"GET".into(),headers:vec![header("User-Agent",USER_AGENT),header("Accept","*/*"),header("Referer",referer)],body:Vec::new(),max_response_bytes:limit,timeout_ms:3_000}
}
pub fn post(url:impl Into<String>,referer:&str,limit:u32)->Request {
    Request{url:url.into(),method:"POST".into(),headers:vec![header("User-Agent",USER_AGENT),header("Accept","*/*"),header("Referer",referer)],body:Vec::new(),max_response_bytes:limit,timeout_ms:3_000}
}
pub fn checked_response(request:&Request,response:Response)->Result<Response,AdapterError>{
    if request.max_response_bytes>0&&response.body.len()>request.max_response_bytes as usize{return Err(AdapterError::Protocol("response byte limit"));}
    if response.url!=request.url{return Err(AdapterError::Protocol("upstream endpoint changed"));}
    match response.status {200=>Ok(response),404|410=>Err(AdapterError::Unavailable("upstream resource")),_=>Err(AdapterError::Host(HostError::UpstreamFailure))}
}
pub fn fetch(host:&dyn ProviderHost,request:Request)->Result<Response,AdapterError>{
    crate::destination::public_url(&request.url)?;
    let response=host.fetch(request.clone())?;checked_response(&request,response)
}
pub fn batch(host:&dyn ProviderHost,requests:Vec<Request>)->Result<Vec<Result<Response,AdapterError>>,AdapterError>{
    if requests.len()>16{return Err(AdapterError::Protocol("batch request count"));}
    for request in &requests{crate::destination::public_url(&request.url)?;}
    let responses=host.fetch_batch(requests.clone())?;
    if responses.len()!=requests.len(){return Err(AdapterError::Protocol("host batch result count"));}
    Ok(requests.iter().zip(responses).map(|(request,response)|response.map_err(AdapterError::from).and_then(|response|checked_response(request,response))).collect())
}
pub fn result(candidates:Vec<CandidateStream>,mut issues:Vec<ProviderFailure>)->AdapterResult {
    issues.truncate(128);
    AdapterResult{candidates:Some(candidates),issues,resolver_duration:0}
}
pub fn issue(error:&AdapterError,stage:&str)->ProviderFailure{ProviderFailure{category:error.category(),stage:stage.into(),detail:"provider record rejected".into()}}
pub fn provider_id(value:&str)->Result<ProviderId,AdapterError>{ProviderId::new(value).map_err(|_|AdapterError::Unsupported)}
pub fn hash_id(parts:&[&[u8]])->String{stream_provider_codecs::sha256(parts).iter().take(12).map(|byte|format!("{byte:02x}")).collect()}
#[allow(dead_code)]
pub fn headers_map(values:Vec<Header>)->Result<BTreeMap<String,String>,AdapterError>{
    if values.len()>64{return Err(AdapterError::Protocol("header count"));}
    let mut result=BTreeMap::new();
    for value in values {
        let name=value.name.to_ascii_lowercase();
        if result.insert(name,value.value).is_some(){return Err(AdapterError::Protocol("duplicate request header"));}
    }
    Ok(result)
}
pub fn same_origin(url:&Url,expected:&str)->bool{Url::parse(expected).is_ok_and(|expected|expected.origin()==url.origin())}
pub fn opaque(value:&str,max:usize)->bool{!value.is_empty()&&value.len()<=max&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'-'|b'_'))}
pub fn nullable_vec<'de,D,T>(deserializer:D)->Result<Vec<T>,D::Error>where D:serde::Deserializer<'de>,T:serde::Deserialize<'de>{Ok(<Option<Vec<T>> as serde::Deserialize>::deserialize(deserializer)?.unwrap_or_default())}
