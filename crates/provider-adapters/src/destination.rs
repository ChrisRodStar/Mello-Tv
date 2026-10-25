//! Pure declaration checks. The native host repeats these checks and validates
//! and pins DNS at connection time; guest checks cannot approve DNS answers.
use std::net::IpAddr;
use url::{Host,Url};
use crate::AdapterError;

fn public_ip(ip:IpAddr)->bool{
    match ip {
        IpAddr::V4(ip)=>{let value=u32::from(ip);![(0,8),(0x0a000000,8),(0x64400000,10),(0x7f000000,8),(0xa9fe0000,16),(0xac100000,12),(0xc0000000,24),(0xc0000200,24),(0xc0586300,24),(0xc0a80000,16),(0xc6120000,15),(0xc6336400,24),(0xcb007100,24),(0xe0000000,3)].iter().any(|&(prefix,bits)|value>>(32-bits)==prefix>>(32-bits))},
        IpAddr::V6(ip)=>{let value=u128::from(ip);value>>125==1&&![(0x2001u128<<112,23),(0x20010db8u128<<96,32),(0x2002u128<<112,16),(0x3fffu128<<112,20)].iter().any(|&(prefix,bits)|value>>(128-bits)==prefix>>(128-bits))},
    }
}

pub fn public_url(raw:&str)->Result<Url,AdapterError>{
    if raw.is_empty()||raw.len()>8192||raw.trim()!=raw||raw.bytes().any(|byte|byte.is_ascii_control()||byte==b'\\'){return Err(AdapterError::Protocol("invalid destination"));}
    let authority=raw.split_once("://").and_then(|(_,rest)|rest.split(['/', '?', '#']).next()).filter(|value|!value.is_empty()).ok_or(AdapterError::Protocol("destination authority"))?;
    if authority.contains(['@','%']){return Err(AdapterError::Protocol("destination credentials"));}
    let url=Url::parse(raw).map_err(|_|AdapterError::Protocol("destination URL"))?;
    if !matches!(url.scheme(),"http"|"https")||url.fragment().is_some()||!url.username().is_empty()||url.password().is_some()||url.port_or_known_default()!=Some(if url.scheme()=="https"{443}else{80}){return Err(AdapterError::Protocol("destination transport"));}
    match url.host().ok_or(AdapterError::Protocol("destination host"))? {
        Host::Ipv4(ip) if public_ip(ip.into())=>{if authority.split(':').next()!=Some(ip.to_string().as_str()){return Err(AdapterError::Protocol("ambiguous address"));}},
        Host::Ipv6(ip) if public_ip(ip.into())=>{},
        Host::Domain(host)=>{let host=host.trim_end_matches('.');if !host.contains('.')||host.split('.').any(str::is_empty)||["localhost","local","internal","home","lan","test","invalid","onion"].iter().any(|suffix|host==*suffix||host.ends_with(&format!(".{suffix}"))){return Err(AdapterError::Protocol("nonpublic hostname"));}},
        _=>return Err(AdapterError::Protocol("nonpublic address")),
    }
    Ok(url)
}
