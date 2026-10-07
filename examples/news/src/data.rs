//! Deterministic editorial fixtures; every article is fictional.

#[derive(Clone, Copy, Debug)]
pub struct Story {
    pub id: usize,
    pub title: &'static str,
    pub category: &'static str,
    pub summary: &'static str,
    pub photo: u32,
}

pub const CATEGORIES: [&str; 5] = ["For you", "Culture", "Planet", "Design", "Travel"];
pub const STORY_COUNT: usize = 600;

const EDITION: [(&str, &str, &str, u32); 12] = [
    (
        "The quiet places shaping our future",
        "Planet",
        "森と暮らしの新しい関係。小さな地域から始まる、大きな変化を訪ねて。",
        10,
    ),
    (
        "A different way to see the world",
        "Travel",
        "急がない旅が教えてくれたこと。風景の向こうにある日常を見つめる。",
        11,
    ),
    (
        "Beyond the blue horizon",
        "Planet",
        "海を守る人々と過ごした一週間。次の世代に残したい、かけがえのない景色。",
        12,
    ),
    (
        "Where the landscape becomes a home",
        "Design",
        "自然の輪郭を生かした建築。素材と光から考える、これからの住まい。",
        13,
    ),
    (
        "The art of paying attention",
        "Culture",
        "いつもの風景を、少し違う角度から。観察することから生まれる創造性。",
        16,
    ),
    (
        "Small journeys, lasting impressions",
        "Travel",
        "地図に載らない寄り道へ。街の記憶を受け継ぐ店と、人々の物語。",
        17,
    ),
    (
        "A new chapter for the great outdoors",
        "Planet",
        "自然の中で働き、遊び、学ぶ。持続可能な暮らしを実践する人たち。",
        28,
    ),
    (
        "Built to belong",
        "Design",
        "地域に根ざしたデザインとは何か。使い続けることで育つ道具と空間。",
        29,
    ),
    (
        "Making space for slower days",
        "Culture",
        "忙しさの先にある豊かさ。手を動かし、時間をかけることの価値。",
        42,
    ),
    (
        "Notes from the edge of the map",
        "Travel",
        "旅の終わりに残るもの。遠い場所で出会った、身近な問い。",
        49,
    ),
    (
        "Tomorrow starts in our neighbourhood",
        "Design",
        "歩いて暮らせる街をつくる。共有する場所から始まる新しいコミュニティ。",
        54,
    ),
    (
        "Finding a common ground",
        "Culture",
        "違いを越えて語り合うために。ものづくりがつなぐ、世代と地域。",
        58,
    ),
];

pub fn story(id: usize) -> Story {
    let (title, category, summary, photo) = EDITION[id % EDITION.len()];
    Story {
        id,
        title,
        category,
        summary,
        photo,
    }
}

pub fn photo(id: u32) -> String {
    whisker_asset::resolve(&format!("photos/{id}.jpg"))
}

pub const PARAGRAPHS: [&str; 4] = [
    "朝の光が街に届くころ、私たちは小さな工房を訪ねた。窓の向こうには、長い時間をかけて育まれた風景が広がっている。ここで大切にされているのは、新しさだけではない。土地の記憶を受け取り、次の世代へ手渡すこと。その営みを支えている人々の言葉に耳を傾けた。",
    "Good design starts with a question, not an answer. What do we want to keep? What can we share? Across the region, a growing network of makers is finding possibilities in the things we already have. Their work is patient, practical, and quietly optimistic.",
    "変化は一度には訪れない。毎日の選択を少しずつ見直すことで、暮らしの輪郭が変わっていく。近くでつくられたものを選ぶこと。壊れた道具を修理すること。誰かと食卓を囲むこと。小さな行動の積み重ねが、新しい日常をつくっている。",
    "There is no single blueprint for the future. In the conversations gathered here, the same idea returns: a place thrives when the people who use it can help shape it. That means making room for experiment, disagreement, and the unexpected connections that follow.",
];
