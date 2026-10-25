//! Offline translations of Go VidCore vectors. Challenge/ticket constants were
//! independently calculated from the translated algorithm with Python and
//! OpenSSL AES-CBC; no Go process or live provider is involved.

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use stream_provider_codecs::{
    CryptoError, MAX_PROTOCOL_BYTES, sha256,
    vidcore::{
        MAX_CHALLENGE_RESPONSE_BYTES, MAX_PLAYER_BYTES, MAX_SESSION_BYTES, Protocol, ProtocolError,
        ProtocolParameters, URL_ALPHABET, decode_program, extract_csrf_token, parse_material,
        permutation, string_constant,
    },
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn parameters() -> ProtocolParameters {
    ProtocolParameters {
        base: "/fixture".into(),
        challenge_action: "challenge".into(),
        list_action: "list".into(),
        unlock_action: "unlock".into(),
        csrf_token: "private-csrf-token".into(),
        key: [1; 32],
        iv: [2; 16],
        seed: b"seed".to_vec(),
        response_seed: [9; 32],
        response_numbers: vec![123, 456],
        alphabet: *URL_ALPHABET,
        stage_keys: std::array::from_fn(|index| [u8::try_from(index + 1).unwrap_or_default(); 32]),
        stage_prefixes: std::array::from_fn(|index| {
            vec![u8::try_from(index).unwrap_or_default(), 165]
        }),
        rc4_keys: std::array::from_fn(|index| [u8::try_from(index + 11).unwrap_or_default(); 32]),
    }
}

fn masks() -> &'static str {
    "a=0x1,r=0;r<256;r++){var one=a=a+0x2>>>0;a=0x1,r=0;r<256;r++){var two=a=a+0x2>>>0;"
}

fn envelope(
    parameters: &ProtocolParameters,
    body: &[u8],
    include_prefix: bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut seed = parameters.response_seed.to_vec();
    for number in &parameters.response_numbers {
        seed.extend_from_slice(&number.to_le_bytes());
    }
    let salt = [3u8; 16];
    let nonce = [4u8; 12];
    let key = sha256(&[&sha256(&[&seed]), &salt]);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "cipher material")?;
    let mut plain = if include_prefix {
        vec![0; 8]
    } else {
        Vec::new()
    };
    plain.extend_from_slice(body);
    let encrypted = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &plain,
                aad: &[],
            },
        )
        .map_err(|_| "fixture encryption")?;
    let mut envelope = salt.to_vec();
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&encrypted);
    Ok(envelope)
}

#[test]
fn original_go_material_structure_vector_is_parsed_without_execution() -> TestResult {
    let source = include_str!("../testdata/providers/vidcore/structure.txt");
    let material = parse_material(source)?;
    assert_eq!(material.base, "/fixture");
    assert_eq!(material.action, "unlock");
    assert_eq!(material.require_name, "req");
    assert_eq!(material.crypto_module, "1");
    assert_eq!(material.buffer_module, "2");
    assert!(matches!(
        Protocol::from_source(source),
        Err(ProtocolError::UnknownMaterial)
    ));
    assert!(matches!(
        parse_material("var changed=1;"),
        Err(ProtocolError::UnsupportedStructure)
    ));
    Ok(())
}

#[test]
fn unknown_player_material_never_bypasses_fingerprint_verification() -> TestResult {
    let source = include_str!("../testdata/providers/vidcore/structure.txt");
    for changed in [
        source.replace("/fixture", "//unexpected"),
        source.replace("buf=req(2)", "buf=other(2)"),
        source.replace(",()=>{clearInterval(", ",()=>{changed("),
        source.replace(";await promise,player.current=", ";await changed;"),
    ] {
        assert!(Protocol::from_source(&changed).is_err());
    }
    assert!(matches!(
        Protocol::from_source(&"x".repeat(MAX_PLAYER_BYTES + 1)),
        Err(ProtocolError::InputLimit)
    ));
    Ok(())
}

#[test]
fn static_blob_codecs_and_switch_order_match_the_go_decoder() -> TestResult {
    let prefix = concat!(
        "b64=[99,96,114,100,55,53].map(function(n){return String.fromCharCode(1^n)}).join(\"\");",
        "hx=\"ify\".split(\"\").map(function(n){return String.fromCharCode(n.charCodeAt(0)-1)}).join(\"\");",
        "first=new Uint8Array([1,2]);second=B.from(\"AwQ=\",b64);third=B.from(\"0506f\",hx);",
        "fourth=function(e,t){var z=B.from(e,b64);z[0]^=1;return z}(\"Bwg=\",0);"
    );
    let stage = format!(
        "case 0:parts[0]=third;case 1:parts[1]=first;case 2:parts[2]=fourth;case 3:parts[3]=second;{}",
        masks()
    );
    assert_eq!(decode_program(prefix, &stage, 4)?, [5, 6, 1, 2, 6, 9, 3, 4]);
    Ok(())
}

#[test]
fn murmur_unmasking_wraps_exactly_at_the_go_uint32_boundary() -> TestResult {
    let prefix = "bytes=new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]);";
    let stage = "case 0:parts[0]=bytes;a=0xffffffff,r=0;r<256;r++){var one=a=a+0x9e3779b9>>>0;a=0x12345678,r=0;r<256;r++){var two=a=a+0x6a09e667>>>0;";
    assert_eq!(
        decode_program(prefix, stage, 1)?,
        [
            92, 34, 57, 159, 73, 220, 68, 172, 255, 36, 29, 161, 160, 172, 130, 145
        ]
    );
    Ok(())
}

#[test]
fn missing_duplicate_and_out_of_range_bytecode_parts_fail_closed() -> TestResult {
    let prefix = "part=new Uint8Array([1,2,3]);";
    for stage in [
        format!("case 0:parts[0]=part;case 1:parts[0]=part;{}", masks()),
        format!("case 0:parts[1]=part;{}", masks()),
        format!("case 0:parts[0]=missing;{}", masks()),
        "case 0:parts[0]=part;".to_owned(),
        format!(
            "case 0:parts[0]=part;{}a=0x3,r=0;r<256;r++){{var three=a=a+0x4>>>0;",
            masks()
        ),
    ] {
        assert!(decode_program(prefix, &stage, 1).is_err());
    }
    assert!(decode_program(prefix, &format!("case 0:parts[0]=part;{}", masks()), 2).is_err());
    assert!(
        decode_program(
            "part=new Uint8Array([256]);",
            &format!("case 0:parts[0]=part;{}", masks()),
            1
        )
        .is_err()
    );
    assert!(decode_program(prefix, masks(), 0).is_err());
    assert!(decode_program(prefix, masks(), 65).is_err());
    assert!(decode_program(&"x".repeat(MAX_PLAYER_BYTES + 1), masks(), 1).is_err());
    Ok(())
}

#[test]
fn mapped_string_constants_check_known_opcodes_lengths_and_offsets() -> TestResult {
    for opcode in [[103, 188], [89, 4], [72, 105]] {
        let mut bytes = vec![0, 0];
        bytes.extend_from_slice(&opcode);
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(b"key");
        assert_eq!(string_constant(&bytes, 2)?, "key");
        assert!(string_constant(&bytes, 3).is_err());
    }
    for bytes in [
        &[103, 188, 0, 5, b'a'][..],
        &[0, 0, 0, 0][..],
        &[103, 188, 0, 1, 255][..],
    ] {
        assert!(string_constant(bytes, 0).is_err());
    }
    assert!(string_constant(&[], usize::MAX).is_err());
    Ok(())
}

#[test]
fn csrf_token_is_dynamic_bounded_and_not_a_hardcoded_constant() -> TestResult {
    assert_eq!(
        extract_csrf_token(r#"headers:{"X-Csrf-Token":"fresh-one"}"#)?,
        Some("fresh-one".into())
    );
    assert_eq!(
        extract_csrf_token("headers:{'X-Csrf-Token' : 'fresh-two'}")?,
        Some("fresh-two".into())
    );
    assert_eq!(extract_csrf_token("headers:{}")?, None);
    assert!(extract_csrf_token(r#""X-Csrf-Token":"one";"X-Csrf-Token":"two""#).is_err());
    assert!(extract_csrf_token(&format!("\"X-Csrf-Token\":\"{}\"", "a".repeat(4097))).is_err());
    assert!(extract_csrf_token("\"X-Csrf-Token\":\"bad\nvalue\"").is_err());
    Ok(())
}

#[test]
fn permutation_matches_independent_vector_and_go_uniqueness_bounds() -> TestResult {
    assert_eq!(
        permutation(16, b"synthetic seed")?,
        [3, 6, 0, 13, 8, 15, 11, 4, 14, 1, 9, 2, 10, 12, 5, 7]
    );
    for count in [0, 1, 16, 256, 513] {
        let mut order = permutation(count, b"synthetic seed")?;
        order.sort_unstable();
        assert_eq!(order, (0..count).collect::<Vec<_>>());
    }
    assert!(permutation(MAX_PROTOCOL_BYTES + 1, b"seed").is_err());
    Ok(())
}

#[test]
fn complete_challenge_matches_independent_openssl_and_python_vector() -> TestResult {
    let protocol = Protocol::from_parameters(parameters())?;
    let nonce: [u8; 16] = std::array::from_fn(|index| u8::try_from(index).unwrap_or_default());
    let result = protocol.challenge("synthetic-session", 1_700_000_000_123, &nonce)?;
    assert_eq!(
        result,
        "AQABAgMEBQYHCAkKCwwNDg8DAAAAAAABAAAAAgAAAIp19oqmuKoco5M_PDmc6UMxxmLVKVE-4oIlGvIZzaEdnbP10ZqGDm3CQaeB5aXA1iJUzxxjxGsv"
    );
    assert_ne!(protocol.challenge("synthetic-session", -1, &nonce)?, result);
    assert!(protocol.challenge("", 0, &nonce).is_err());
    assert!(protocol.challenge("session", 0, &[0; 15]).is_err());
    assert!(
        protocol
            .challenge(&"a".repeat(MAX_SESSION_BYTES + 1), 0, &nonce)
            .is_err()
    );
    Ok(())
}

#[test]
fn custom_alphabet_is_a_checked_bijection_applied_after_url_base64() -> TestResult {
    let original = Protocol::from_parameters(parameters())?.challenge("session", 123, &[0; 16])?;
    let mut custom = parameters();
    custom.alphabet.rotate_left(1);
    let result = Protocol::from_parameters(custom)?.challenge("session", 123, &[0; 16])?;
    let translated: Vec<_> = original
        .bytes()
        .map(|symbol| {
            URL_ALPHABET
                .iter()
                .position(|source| *source == symbol)
                .and_then(|index| URL_ALPHABET.get((index + 1) % 64))
                .copied()
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(result.as_bytes(), translated);
    let mut duplicate = parameters();
    duplicate.alphabet[0] = duplicate.alphabet[1];
    assert!(Protocol::from_parameters(duplicate).is_err());
    let mut control = parameters();
    control.alphabet[0] = b'\n';
    assert!(Protocol::from_parameters(control).is_err());
    Ok(())
}

#[test]
fn complete_ticket_matches_independent_go_pipeline_translation() -> TestResult {
    let protocol = Protocol::from_parameters(parameters())?;
    assert_eq!(
        protocol.ticket(b"challenge-response")?,
        "HmkReqNfbDpoR6vrRJrIQRBEUs_BZUDaxlIaVLGdujTLTqQ2pqNWVgNZpO0sWA"
    );
    assert!(protocol.ticket("non-ASCII-é".as_bytes()).is_err());
    assert!(
        protocol
            .ticket(&vec![b'a'; MAX_CHALLENGE_RESPONSE_BYTES + 1])
            .is_err()
    );
    Ok(())
}

#[test]
fn original_go_response_envelope_authentication_and_bounds_are_preserved() -> TestResult {
    let parameters = parameters();
    let protocol = Protocol::from_parameters(parameters.clone())?;
    let expected = br#"{"url":"https://example.com/fixture.m3u8"}"#;
    let mut envelope = envelope(&parameters, expected, true)?;
    let encoded = STANDARD.encode(&envelope);
    assert_eq!(
        protocol.decode_response(encoded.as_bytes())?.as_slice(),
        expected
    );
    assert_eq!(
        protocol
            .decode_response(STANDARD_NO_PAD.encode(&envelope).as_bytes())?
            .as_slice(),
        expected
    );
    assert_eq!(
        protocol
            .decode_response(format!(" \n{encoded}\r\n").as_bytes())?
            .as_slice(),
        expected
    );
    let last = envelope.last_mut().ok_or("fixture envelope empty")?;
    *last ^= 1;
    assert!(matches!(
        protocol.decode_response(STANDARD.encode(&envelope).as_bytes()),
        Err(ProtocolError::Crypto(CryptoError::Authentication))
    ));
    for malformed in [
        String::new(),
        "not base64".to_owned(),
        STANDARD.encode([0; 43]),
    ] {
        assert!(protocol.decode_response(malformed.as_bytes()).is_err());
    }
    assert!(
        protocol
            .decode_response(&vec![b'a'; MAX_PROTOCOL_BYTES + 1])
            .is_err()
    );
    Ok(())
}

#[test]
fn authenticated_but_truncated_plaintext_cannot_lose_its_required_prefix() -> TestResult {
    let parameters = parameters();
    let protocol = Protocol::from_parameters(parameters.clone())?;
    let envelope = envelope(&parameters, b"short", false)?;
    assert!(matches!(
        protocol.decode_response(STANDARD.encode(envelope).as_bytes()),
        Err(ProtocolError::Invalid("response plaintext prefix"))
    ));
    Ok(())
}

#[test]
fn low_level_material_rejects_bad_dimensions_and_redacts_credentials() -> TestResult {
    let mut empty_seed = parameters();
    empty_seed.seed.clear();
    assert!(Protocol::from_parameters(empty_seed).is_err());
    let mut bad_base = parameters();
    bad_base.base = "//other.example/".into();
    assert!(Protocol::from_parameters(bad_base).is_err());
    let mut bad_action = parameters();
    bad_action.unlock_action.clear();
    assert!(Protocol::from_parameters(bad_action).is_err());
    let parameters = parameters();
    assert!(!format!("{parameters:?}").contains("private-csrf-token"));
    let protocol = Protocol::from_parameters(parameters)?;
    assert_eq!(protocol.base(), "/fixture");
    assert_eq!(protocol.challenge_action(), "challenge");
    assert_eq!(protocol.list_action(), "list");
    assert_eq!(protocol.unlock_action(), "unlock");
    assert_eq!(protocol.csrf_token(), "private-csrf-token");
    assert!(protocol.version().is_none());
    assert!(!format!("{protocol:?}").contains("private-csrf-token"));
    Ok(())
}
