use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QList, QMap, QMapPair_QString_QVariant,
    QModelIndex, QString, QVariant, QVector,
};
use serde_json::{Map, Value};

use crate::bridge::qobject::{FeedListModel, ItemListModel};

pub type Row = Map<String, Value>;

pub const FEED_KEYS: [&str; 11] = [
    "id", "title", "unread", "web", "icon", "error", "notify", "url", "site", "folder", "full",
];
pub const ITEM_KEYS: [&str; 14] = [
    "id", "feed_id", "title", "feed", "preview", "image", "when", "day", "read", "starred", "link",
    "web", "media", "hidden",
];

const USER_ROLE: i32 = 0x0100;

pub fn id(r: &Row) -> i64 {
    r["id"].as_i64().unwrap_or(0)
}

#[derive(Default)]
pub struct FeedRows {
    pub rows: Vec<Row>,
}

#[derive(Default)]
pub struct ItemRows {
    pub rows: Vec<Row>,
}

pub fn to_qvariant(v: &Value) -> QVariant {
    match v {
        Value::Null => QVariant::default(),
        Value::Bool(b) => QVariant::from(b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i32::try_from(i).map_or_else(|_| QVariant::from(&i), |i| QVariant::from(&i)),
            None => QVariant::from(&n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => QVariant::from(&QString::from(s.as_str())),
        Value::Array(a) => {
            let mut list = QList::<QVariant>::default();
            for item in a {
                list.append(to_qvariant(item));
            }
            QVariant::from(&list)
        }
        Value::Object(o) => {
            let mut map = QMap::<QMapPair_QString_QVariant>::default();
            for (k, item) in o {
                map.insert(QString::from(k.as_str()), to_qvariant(item));
            }
            QVariant::from(&map)
        }
    }
}

macro_rules! list_model {
    ($ty:ty, $keys:expr) => {
        impl $ty {
            pub fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
                let key = usize::try_from(role - USER_ROLE)
                    .ok()
                    .and_then(|i| $keys.get(i));
                match (self.rows.get(index.row() as usize), key) {
                    (Some(r), Some(k)) => r.get(*k).map(to_qvariant).unwrap_or_default(),
                    _ => QVariant::default(),
                }
            }

            pub fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
                let mut h = QHash::<QHashPair_i32_QByteArray>::default();
                for (i, k) in $keys.iter().enumerate() {
                    h.insert(USER_ROLE + i as i32, QByteArray::from(*k));
                }
                h
            }

            pub fn row_count(&self, _parent: &QModelIndex) -> i32 {
                self.rows.len() as i32
            }

            pub fn find(&self, rid: i64) -> Option<usize> {
                self.rows.iter().position(|r| id(r) == rid)
            }

            pub fn changed_at(mut self: Pin<&mut Self>, i: usize) {
                let idx = self.index(i as i32, 0, &QModelIndex::default());
                self.as_mut().data_changed(&idx, &idx, &QVector::default());
            }

            pub fn set(mut self: Pin<&mut Self>, rows: Vec<Row>, reset: bool) {
                let ids: Vec<i64> = rows.iter().map(id).collect();
                let old: Vec<i64> = self.rows.iter().map(id).collect();
                let kept: Vec<i64> = old.iter().copied().filter(|i| ids.contains(i)).collect();
                let order: Vec<i64> = ids.iter().copied().filter(|i| old.contains(i)).collect();
                if reset || kept != order {
                    self.as_mut().begin_reset_model();
                    self.as_mut().rust_mut().rows = rows;
                    self.as_mut().end_reset_model();
                    return;
                }
                let root = QModelIndex::default();
                for i in (0..old.len()).rev() {
                    if !ids.contains(&old[i]) {
                        self.as_mut().begin_remove_rows(&root, i as i32, i as i32);
                        self.as_mut().rust_mut().rows.remove(i);
                        self.as_mut().end_remove_rows();
                    }
                }
                for (i, r) in rows.into_iter().enumerate() {
                    if i == self.rows.len() || id(&self.rows[i]) != id(&r) {
                        self.as_mut().begin_insert_rows(&root, i as i32, i as i32);
                        self.as_mut().rust_mut().rows.insert(i, r);
                        self.as_mut().end_insert_rows();
                    } else if self.rows[i] != r {
                        self.as_mut().rust_mut().rows[i] = r;
                        self.as_mut().changed_at(i);
                    }
                }
            }
        }
    };
}

list_model!(FeedListModel, FEED_KEYS);
list_model!(ItemListModel, ITEM_KEYS);
