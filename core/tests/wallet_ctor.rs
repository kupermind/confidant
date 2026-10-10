//! Every wallet constructor is exercised directly (the browser build reaches them through
//! wasm-bindgen; coverage must not depend on which binary happens to inline which one).
use confidant_core::Wallet;

#[test]
fn constructors_build_equivalent_wallets() {
    let a = Wallet::new();
    let b = Wallet::default();
    assert_ne!(a.info().address, b.info().address, "fresh keys differ");
    let secret = a.secret();
    assert_eq!(secret.format, 1);
    let c = Wallet::from_secret(&secret).unwrap();
    assert_eq!(a.info().address, c.info().address);
    assert_eq!(a.info().viewing_key, c.info().viewing_key);
    let raw: [u8; 32] = hex::decode(&secret.spending_key)
        .unwrap()
        .try_into()
        .unwrap();
    let sk = Option::from(confidant_core::orchard::keys::SpendingKey::from_bytes(raw)).unwrap();
    let d = Wallet::from_spending_key(sk);
    assert_eq!(a.info().viewing_key, d.info().viewing_key);
    assert_eq!(d.balance(), 0);
    assert_eq!(d.synced(), 0);
}
