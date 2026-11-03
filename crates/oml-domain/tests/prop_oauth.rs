//! Properties of ADR-0031's pure modules: a parsed connect code round-trips
//! (d8), no string is both a token and a key (d10), and under rotation any
//! rotated refresh token presented again revokes its connection, once, after
//! which nothing rotates (d10).

use oml_domain::oauth::{
    ConnectCode, ConnectionState, Issuer, KeyState, RefreshDecision, RefreshRow, Token, TokenKind,
    base62_43, connection_transition, decide_refresh, is_key_value, token_kind,
};
use proptest::prelude::*;

const SIXTY_BITS: u64 = (1 << 60) - 1;

/// How one symbol of an entry is typed: as shown, or with the misreadings
/// and separators Crockford's decoding rule ignores.
fn arb_typing() -> impl Strategy<Value = Vec<(bool, u8, u8)>> {
    proptest::collection::vec((any::<bool>(), 0..3_u8, 0..3_u8), 12)
}

proptest! {
    #[test]
    fn a_parsed_code_round_trips(random in any::<[u8; 8]>(), typing in arb_typing()) {
        let code = ConnectCode::from_random(random);
        prop_assert_eq!(ConnectCode::parse(&code.grouped()), Ok(code.clone()));
        prop_assert_eq!(ConnectCode::parse(code.symbols()), Ok(code.clone()));

        let mut entry = String::new();
        for (symbol, (lower, misread, separator)) in code.symbols().chars().zip(typing) {
            let typed = match (symbol, misread) {
                ('0', 1 | 2) => 'O',
                ('1', 1) => 'I',
                ('1', 2) => 'L',
                _ => symbol,
            };
            entry.push(if lower { typed.to_ascii_lowercase() } else { typed });
            match separator {
                0 => entry.push('-'),
                1 => entry.push(' '),
                _ => {}
            }
        }
        let parsed = ConnectCode::parse(&entry);
        prop_assert_eq!(&parsed, &Ok(code.clone()), "{:?}", entry);
        prop_assert_eq!(parsed.map(|c| c.digest()), Ok(code.digest()));
    }

    #[test]
    fn two_codes_are_equal_exactly_when_their_sixty_bits_are(a in any::<u64>(), b in any::<u64>()) {
        let same = ConnectCode::from_random(a.to_be_bytes()) == ConnectCode::from_random(b.to_be_bytes());
        prop_assert_eq!(same, a & SIXTY_BITS == b & SIXTY_BITS);
    }

    #[test]
    fn no_string_is_both_a_token_and_a_key(
        s in prop_oneof![
            "oml_[A-Za-z0-9_]{38,52}",
            "oml_(at|rt)_[A-Za-z0-9]{40,46}",
            "oml_(at|rt)[A-Za-z0-9_]{38,46}",
            any::<String>(),
        ]
    ) {
        prop_assert!(!(is_key_value(&s) && token_kind(&s).is_some()), "{:?}", s);
    }

    #[test]
    fn a_minted_token_or_key_parses_only_as_itself(random in any::<[u8; 32]>()) {
        let key = format!("oml_{}", base62_43(&random));
        prop_assert!(is_key_value(&key));
        prop_assert_eq!(token_kind(&key), None);
        for kind in [TokenKind::Access, TokenKind::Refresh] {
            let token = Token::mint(kind, random);
            prop_assert_eq!(token_kind(token.as_str()), Some(kind));
            prop_assert!(!is_key_value(token.as_str()));
        }
    }

    #[test]
    fn any_rotated_refresh_token_presented_again_revokes_once_and_nothing_rotates_after(
        presentations in proptest::collection::vec(0..6_usize, 1..32)
    ) {
        let issuer = Issuer::parse("https://oml.example.com").expect("a public origin");
        // The model: the connection's refresh tokens in minting order, each
        // rotated or not, and the connection's state.
        let mut rotated = vec![false];
        let mut state = ConnectionState::Open;
        let mut revocations = 0;
        for index in presentations {
            let Some(&was_rotated) = rotated.get(index) else {
                continue;
            };
            let row = RefreshRow {
                rotated: was_rotated,
                expired: false,
                connection: state,
                client_id: "c_client",
                resource: issuer.resource(),
                key: KeyState::Active,
            };
            let decision = decide_refresh(Some(&row), "c_client", &issuer);
            match (state, was_rotated) {
                (ConnectionState::Open, false) => {
                    prop_assert_eq!(decision, RefreshDecision::Rotate);
                    rotated[index] = true;
                    rotated.push(false);
                }
                (ConnectionState::Open, true) => {
                    prop_assert_eq!(decision, RefreshDecision::RevokeForReuse);
                    let act = decision.act().expect("a revocation");
                    state = connection_transition(state, act).expect("open");
                    revocations += 1;
                }
                _ => prop_assert!(matches!(decision, RefreshDecision::Refuse(_))),
            }
            // While open, exactly one refresh token is unrotated: the newest.
            if state == ConnectionState::Open {
                prop_assert_eq!(rotated.iter().filter(|r| !**r).count(), 1);
                prop_assert_eq!(rotated.last(), Some(&false));
            }
        }
        prop_assert!(revocations <= 1);
    }
}
