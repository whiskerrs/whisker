use crate::native::{self, Source};
use crate::reference::{validate_collection_id, validate_collection_path};
use crate::snapshot::{QuerySnapshot, SnapshotOptions};
use crate::{FirebaseError, ListenerRegistration, Result, Value};
use whisker::ReadSignal;

/// Comparison used by [`Query::where_field`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterOp {
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    ArrayContains,
    /// The value must be a non-empty array of at most 30 elements.
    ArrayContainsAny,
    /// The value must be a non-empty array of at most 30 elements.
    In,
    /// The value must be a non-empty array of at most 10 elements.
    NotIn,
}

impl FilterOp {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FilterOp::Equal => "==",
            FilterOp::NotEqual => "!=",
            FilterOp::LessThan => "<",
            FilterOp::LessThanOrEqual => "<=",
            FilterOp::GreaterThan => ">",
            FilterOp::GreaterThanOrEqual => ">=",
            FilterOp::ArrayContains => "array-contains",
            FilterOp::ArrayContainsAny => "array-contains-any",
            FilterOp::In => "in",
            FilterOp::NotIn => "not-in",
        }
    }

    fn array_limit(self) -> Option<usize> {
        match self {
            FilterOp::ArrayContainsAny | FilterOp::In => Some(30),
            FilterOp::NotIn => Some(10),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Target {
    Collection(String),
    Group(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Filter {
    pub field: String,
    pub op: FilterOp,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cursor {
    pub values: Vec<Value>,
    pub inclusive: bool,
}

/// A filtered, ordered view of a collection or collection group. Builder methods
/// return a new query; invalid queries are reported by `get`/`count`/`on_snapshot`.
///
/// ```ignore
/// let top = db.collection("scores")
///     .where_field("level", FilterOp::GreaterThanOrEqual, 3)
///     .order_by("points", Direction::Descending)
///     .limit(10)
///     .get()
///     .await?;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub(crate) target: Target,
    pub(crate) filters: Vec<Filter>,
    pub(crate) order_by: Vec<(String, Direction)>,
    pub(crate) limit: Option<(u32, bool)>,
    pub(crate) start: Option<Cursor>,
    pub(crate) end: Option<Cursor>,
    error: Option<FirebaseError>,
}

impl Query {
    fn new(target: Target) -> Self {
        Self {
            target,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            start: None,
            end: None,
            error: None,
        }
    }

    pub(crate) fn collection(path: String) -> Self {
        Self::new(Target::Collection(path))
    }

    pub(crate) fn group(id: String) -> Self {
        Self::new(Target::Group(id))
    }

    fn with(&self, change: impl FnOnce(&mut Self) -> Result<()>) -> Self {
        let mut query = self.clone();
        if query.error.is_none()
            && let Err(error) = change(&mut query)
        {
            query.error = Some(error);
        }
        query
    }

    /// Keep documents whose `field` (a dotted path) satisfies `op value`.
    pub fn where_field(&self, field: &str, op: FilterOp, value: impl Into<Value>) -> Self {
        let value = value.into();
        self.with(|query| {
            validate_field_path(field)?;
            if let Some(max) = op.array_limit() {
                match &value {
                    Value::Array(values) if !values.is_empty() && values.len() <= max => {}
                    _ => {
                        return Err(invalid(format!(
                            "`{}` needs a non-empty array of at most {max} values",
                            op.as_str()
                        )));
                    }
                }
            }
            let is_nan = matches!(value, Value::Double(v) if v.is_nan());
            if (value.is_null() || is_nan) && !matches!(op, FilterOp::Equal | FilterOp::NotEqual) {
                return Err(invalid("null and NaN only support `==` and `!=`"));
            }
            let has = |op| query.filters.iter().any(|f| f.op == op);
            if op == FilterOp::NotIn && (has(FilterOp::NotIn) || has(FilterOp::NotEqual))
                || op == FilterOp::NotEqual && has(FilterOp::NotIn)
            {
                return Err(invalid(
                    "`not-in` cannot be combined with another `not-in` or `!=` filter",
                ));
            }
            query.filters.push(Filter {
                field: field.into(),
                op,
                value,
            });
            Ok(())
        })
    }

    /// Sort by `field` (a dotted path). Documents missing the field are excluded.
    pub fn order_by(&self, field: &str, direction: Direction) -> Self {
        self.with(|query| {
            validate_field_path(field)?;
            query.order_by.push((field.into(), direction));
            Ok(())
        })
    }

    /// Return at most the first `count` documents.
    pub fn limit(&self, count: u32) -> Self {
        self.with(|query| {
            query.limit = Some((count, false));
            Ok(())
        })
    }

    /// Return at most the last `count` documents; requires an `order_by`.
    pub fn limit_to_last(&self, count: u32) -> Self {
        self.with(|query| {
            query.limit = Some((count, true));
            Ok(())
        })
    }

    /// Start at documents whose `order_by` fields equal these values (inclusive).
    pub fn start_at<V: Into<Value>>(&self, values: impl IntoIterator<Item = V>) -> Self {
        self.cursor(values, true, true)
    }

    /// Start after documents whose `order_by` fields equal these values.
    pub fn start_after<V: Into<Value>>(&self, values: impl IntoIterator<Item = V>) -> Self {
        self.cursor(values, true, false)
    }

    /// End at documents whose `order_by` fields equal these values (inclusive).
    pub fn end_at<V: Into<Value>>(&self, values: impl IntoIterator<Item = V>) -> Self {
        self.cursor(values, false, true)
    }

    /// End before documents whose `order_by` fields equal these values.
    pub fn end_before<V: Into<Value>>(&self, values: impl IntoIterator<Item = V>) -> Self {
        self.cursor(values, false, false)
    }

    fn cursor<V: Into<Value>>(
        &self,
        values: impl IntoIterator<Item = V>,
        start: bool,
        inclusive: bool,
    ) -> Self {
        let values: Vec<Value> = values.into_iter().map(Into::into).collect();
        self.with(|query| {
            if values.is_empty() {
                return Err(invalid("cursors need at least one value"));
            }
            let cursor = Some(Cursor { values, inclusive });
            if start {
                query.start = cursor;
            } else {
                query.end = cursor;
            }
            Ok(())
        })
    }

    pub(crate) fn validated(&self) -> Result<&Self> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        match &self.target {
            Target::Collection(path) => validate_collection_path(path)?,
            Target::Group(id) => validate_collection_id(id)?,
        }
        if matches!(self.limit, Some((_, true))) && self.order_by.is_empty() {
            return Err(invalid("`limit_to_last` requires at least one `order_by`"));
        }
        for cursor in [&self.start, &self.end].into_iter().flatten() {
            if cursor.values.len() > self.order_by.len() {
                return Err(invalid(
                    "cursors cannot have more values than `order_by` clauses",
                ));
            }
        }
        Ok(self)
    }

    pub async fn get(&self) -> Result<QuerySnapshot> {
        self.get_with(Source::Default).await
    }

    pub async fn get_with(&self, source: Source) -> Result<QuerySnapshot> {
        native::get_query(self.validated()?, source).await
    }

    /// Count matching documents on the server without downloading them.
    pub async fn count(&self) -> Result<u64> {
        native::count(self.validated()?).await
    }

    /// Receive the current results and every later change until the registration drops.
    pub fn on_snapshot(
        &self,
        callback: impl Fn(Result<QuerySnapshot>) + 'static,
    ) -> Result<ListenerRegistration> {
        self.on_snapshot_with(SnapshotOptions::default(), callback)
    }

    pub fn on_snapshot_with(
        &self,
        options: SnapshotOptions,
        callback: impl Fn(Result<QuerySnapshot>) + 'static,
    ) -> Result<ListenerRegistration> {
        native::listen_query(self.validated()?, options, callback)
    }

    /// The latest results as a signal; `None` until the first snapshot arrives.
    /// Call inside a component: the listener stops when the component is disposed.
    pub fn snapshot_signal(&self) -> ReadSignal<Option<Result<QuerySnapshot>>> {
        let this = self.clone();
        whisker_firebase_core::__private::signal_from_listener(
            None,
            move |set| this.on_snapshot(move |snapshot| set(Some(snapshot))),
            |error| Some(Err(error)),
        )
    }
}

fn invalid(message: impl Into<String>) -> FirebaseError {
    FirebaseError::invalid_argument("firestore", message)
}

/// Dotted field paths as accepted by the SDKs' string overloads.
pub(crate) fn validate_field_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.split('.').any(str::is_empty)
        || path.contains(['~', '*', '/', '[', ']'])
    {
        return Err(invalid(format!("`{path}` is not a valid field path")));
    }
    Ok(())
}
