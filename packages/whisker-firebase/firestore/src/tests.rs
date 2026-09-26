use super::*;
use crate::batch::WriteOp;
use crate::ser::{WriteValue, to_fields, to_write_value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Role {
    Admin,
    Member { since: i64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Profile {
    name: String,
    age: u32,
    nickname: Option<String>,
    created: Timestamp,
    home: GeoPoint,
    avatar: Bytes,
    manager: Option<DocumentReference>,
    roles: Vec<Role>,
    scores: std::collections::BTreeMap<String, f64>,
    extra: Value,
}

fn profile() -> Profile {
    Profile {
        name: "Alice".into(),
        age: 30,
        nickname: None,
        created: Timestamp::new(1_700_000_000, 123_000).unwrap(),
        home: GeoPoint::new(35.0, 139.0),
        avatar: Bytes(vec![0, 1, 255]),
        manager: Some(Firestore { _private: () }.doc("users/bob")),
        roles: vec![Role::Admin, Role::Member { since: 2020 }],
        scores: [("math".to_string(), 9.5)].into(),
        extra: Value::Map(
            [
                (
                    "when".into(),
                    Value::Timestamp(Timestamp::new(-1, 999_999_000).unwrap()),
                ),
                (
                    "type".into(),
                    Value::String("reserved keys stay plain".into()),
                ),
            ]
            .into(),
        ),
    }
}

#[test]
fn structs_round_trip_through_native_values_and_the_wire() {
    let value = to_value(&profile()).unwrap();
    let Value::Map(fields) = &value else { panic!() };
    assert!(matches!(fields["created"], Value::Timestamp(_)));
    assert!(matches!(fields["home"], Value::GeoPoint(_)));
    assert_eq!(fields["avatar"], Value::Bytes(Bytes(vec![0, 1, 255])));
    assert!(matches!(&fields["manager"], Value::Reference(r) if r.path() == "users/bob"));
    assert_eq!(fields["nickname"], Value::Null);
    assert_eq!(
        fields["roles"].as_array().unwrap()[0],
        Value::String("Admin".into())
    );

    let wire = native::encode_for_test(&WriteValue::from(value.clone())).unwrap();
    let decoded = native::decode(wire).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(from_value::<Profile>(decoded.clone()).unwrap(), profile());
    // `Value` itself survives a trip through serde without losing special types.
    assert_eq!(from_value::<Value>(decoded.clone()).unwrap(), decoded);
}

#[test]
fn field_values_are_write_only_and_validated() {
    let fields = to_fields(&data! {
        "count" => FieldValue::increment(1),
        "tags" => FieldValue::array_union(["a", "b"]),
        "at" => FieldValue::server_timestamp(),
    })
    .unwrap();
    assert!(matches!(fields["count"], WriteValue::Transform(_)));
    assert!(to_value(&data! { "at" => FieldValue::server_timestamp() }).is_err());
    assert!(to_write_value(&vec![FieldValue::delete()]).is_err());
    assert!(to_write_value(&vec![vec![1]]).is_err());
    assert!(to_write_value(&u64::MAX).is_err());
    assert!(to_fields(&5).is_err());

    let doc = Firestore { _private: () }.doc("users/alice");
    let delete = data! { "old" => FieldValue::delete() };
    assert!(WriteOp::set(&doc, &delete, SetOptions::overwrite()).is_err());
    assert!(WriteOp::set(&doc, &delete, SetOptions::merge()).is_ok());
    assert!(WriteOp::update(&doc, &delete).is_ok());
    let nested = data! { "a" => data! { "b" => FieldValue::delete() } };
    assert!(WriteOp::update(&doc, &nested).is_err());
    assert!(WriteOp::update(&doc, &data! { "a..b" => 1 }).is_err());
    assert!(WriteOp::update(&doc, &data! {}).is_err());
    let bad_point = data! { "p" => GeoPoint::new(91.0, 0.0) };
    assert!(WriteOp::set(&doc, &bad_point, SetOptions::overwrite()).is_err());
}

#[test]
fn references_and_queries_validate_before_native_dispatch() {
    let db = Firestore { _private: () };
    let alice = db.collection("users").doc("alice");
    assert_eq!(alice.path(), "users/alice");
    assert_eq!(alice.id(), "alice");
    assert_eq!(alice.parent().path(), "users");
    assert_eq!(alice.collection("posts").parent().unwrap(), alice);
    assert!(db.collection("users").parent().is_none());

    for path in [
        "",
        "users",
        "/users/a",
        "users/a/",
        "users//a",
        "users/..",
        "users/__id__",
    ] {
        assert!(reference::validate_document_path(path).is_err(), "{path}");
    }

    let users = db.collection("users");
    assert!(
        users
            .where_field("age", FilterOp::GreaterThan, 18)
            .validated()
            .is_ok()
    );
    assert!(
        users
            .where_field("age", FilterOp::In, 18)
            .validated()
            .is_err()
    );
    assert!(
        users
            .where_field("age", FilterOp::In, Vec::<i64>::new())
            .validated()
            .is_err()
    );
    assert!(
        users
            .where_field("age", FilterOp::LessThan, Value::Null)
            .validated()
            .is_err()
    );
    assert!(
        users
            .where_field("a", FilterOp::NotIn, vec![1])
            .where_field("b", FilterOp::NotEqual, 2)
            .validated()
            .is_err()
    );
    assert!(
        users
            .where_field("a[0]", FilterOp::Equal, 1)
            .validated()
            .is_err()
    );
    assert!(users.limit_to_last(3).validated().is_err());
    assert!(
        users
            .order_by("age", Direction::Ascending)
            .limit_to_last(3)
            .validated()
            .is_ok()
    );
    assert!(
        users
            .order_by("age", Direction::Ascending)
            .start_after([1, 2])
            .validated()
            .is_err()
    );
    assert!(db.collection_group("a/b").validated().is_err());
    assert!(
        db.collection("users/alice")
            .where_field("x", FilterOp::Equal, 1)
            .validated()
            .is_err()
    );
}

#[test]
fn snapshots_decode_fields_by_path() {
    let snapshot = snapshot::DocumentSnapshot {
        reference: DocumentReference::new("users/alice".into()),
        fields: Some(
            [(
                "address".into(),
                Value::Map([("city".into(), Value::String("Tokyo".into()))].into()),
            )]
            .into(),
        ),
        metadata: Default::default(),
    };
    assert_eq!(
        snapshot.get::<String>("address.city").unwrap().as_deref(),
        Some("Tokyo")
    );
    assert_eq!(snapshot.get::<String>("address.zip").unwrap(), None);
    assert!(snapshot.get::<i64>("address.city").is_err());
    #[derive(Deserialize)]
    struct Address {
        city: String,
    }
    #[derive(Deserialize)]
    struct User {
        address: Address,
    }
    assert_eq!(
        snapshot.data::<User>().unwrap().unwrap().address.city,
        "Tokyo"
    );
}
