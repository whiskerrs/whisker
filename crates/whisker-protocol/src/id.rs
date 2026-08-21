//! Stable identifiers carried by the semantic frame protocol.

macro_rules! define_id {
    ($name:ident, $repr:ty, $summary:literal) => {
        #[doc = $summary]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($repr);

        impl $name {
            /// Creates an identifier, returning `None` for the reserved zero value.
            pub const fn new(value: $repr) -> Option<Self> {
                if value == 0 { None } else { Some(Self(value)) }
            }

            /// Returns the integer carried by this identifier.
            pub const fn get(self) -> $repr {
                self.0
            }
        }
    };
}

define_id!(
    SurfaceId,
    u64,
    "Identifies one independently presented surface"
);
define_id!(
    NodeId,
    u64,
    "Identifies one scene node within a scene epoch"
);
define_id!(
    ElementTypeId,
    u32,
    "Identifies an element schema negotiated when a surface attaches"
);
define_id!(
    PropertyId,
    u32,
    "Identifies a typed common-style or element-specific property"
);
define_id!(
    CommandId,
    u32,
    "Identifies a command declared by an element schema"
);
define_id!(
    ResultId,
    u64,
    "Correlates an asynchronous command result with its invocation"
);
define_id!(
    PointerId,
    u64,
    "Identifies one Host pointer stream for capture operations"
);
define_id!(
    MeasurementKey,
    u64,
    "Correlates one intrinsic measurement request with its immediate response"
);
define_id!(
    MeasurementRequestId,
    u64,
    "Correlates a deferred intrinsic measurement with its later completion"
);
define_id!(
    PreparedContentId,
    u64,
    "Identifies Host-prepared content shared by measurement and painting"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_reserved_for_every_id_width() {
        assert_eq!(NodeId::new(0), None);
        assert_eq!(PropertyId::new(0), None);
        assert_eq!(MeasurementKey::new(0), None);
        assert_eq!(MeasurementRequestId::new(0), None);
        assert_eq!(PreparedContentId::new(0), None);
    }

    #[test]
    fn id_round_trips_its_integer() {
        let id = NodeId::new(42).expect("non-zero ID");
        assert_eq!(id.get(), 42);
    }
}
