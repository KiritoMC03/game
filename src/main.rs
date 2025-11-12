use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    extract::State,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

// ===================== Доменные типы =====================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Reaction {
    Lie,
    Delay,
    Freeze,
}

impl Reaction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "lie" => Some(Reaction::Lie),
            "delay" => Some(Reaction::Delay),
            "freeze" => Some(Reaction::Freeze),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct Situation {
    title: String,
    description: String,
    // ключ: (Reaction, Reaction) — отсортирован
    answers: HashMap<(Reaction, Reaction), String>,
}

#[derive(Clone, Serialize)]
struct ShownResult {
    situation_title: String,
    answer: String,
    counts: [u64; 3],
    version: u64,
}

#[derive(Clone)]
struct AppState {
    situations: Vec<Situation>,
    current_index: usize,
    counts: [u64; 3], // [lie, delay, freeze]
    last_result: Option<ShownResult>,
    result_version: u64,
}

type Shared = Arc<Mutex<AppState>>;

// ===================== Entry =====================

#[tokio::main]
async fn main() {
    let situations = build_situations();
    let state = Arc::new(Mutex::new(AppState {
        situations,
        current_index: 0,
        counts: [0, 0, 0],
        last_result: None,
        result_version: 0,
    }));

    let app = Router::new()
        .route("/", get(index_page))
        .route("/admin", get(admin_page))
        .route("/api/current", get(get_current_situation))
        .route("/api/click", post(post_click))
        .route("/api/result", get(get_result_for_players))
        .route("/admin/show", get(admin_show))
        .route("/admin/next", post(admin_next))
        .route("/admin/reset", post(admin_reset))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("Listening on http://{addr}");

    axum::serve(listener, app).await.unwrap();
}

// ===================== Handlers =====================

async fn index_page() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn admin_page() -> Html<&'static str> {
    Html(ADMIN_HTML)
}

#[derive(Serialize)]
struct CurrentSituationResponse {
    title: String,
    description: String,
}

async fn get_current_situation(State(state): State<Shared>) -> Json<CurrentSituationResponse> {
    let st = state.lock().unwrap();
    let s = &st.situations[st.current_index];
    Json(CurrentSituationResponse {
        title: s.title.clone(),
        description: s.description.clone(),
    })
}

#[derive(Deserialize)]
struct ClickRequest {
    reaction: String,
}

#[derive(Serialize)]
struct ClickResponse {
    ok: bool,
}

async fn post_click(
    State(state): State<Shared>,
    Json(payload): Json<ClickRequest>,
) -> Json<ClickResponse> {
    let mut st = state.lock().unwrap();
    if let Some(r) = Reaction::from_str(&payload.reaction) {
        match r {
            Reaction::Lie => st.counts[0] += 1,
            Reaction::Delay => st.counts[1] += 1,
            Reaction::Freeze => st.counts[2] += 1,
        }
    }
    Json(ClickResponse { ok: true })
}

// Админ нажал “Показать ответ”
async fn admin_show(State(state): State<Shared>) -> Json<ShownResult> {
    let mut st = state.lock().unwrap();

    // сначала забираем всё неизменяемое
    let situation = &st.situations[st.current_index];
    let (r1, r2) = top_two(&st.counts);
    let key = ordered_tuple(r1, r2);
    let answer = situation
        .answers
        .get(&key)
        .cloned()
        .unwrap_or_else(|| "Ответ не найден для этой комбинации".to_string());
    let situation_title = situation.title.clone();
    let counts = st.counts;

    // теперь можно мутировать состояние
    st.result_version += 1;
    let shown = ShownResult {
        situation_title,
        answer,
        counts,
        version: st.result_version,
    };
    st.last_result = Some(shown.clone());

    Json(shown)
}

// игроки опрашивают результат
async fn get_result_for_players(State(state): State<Shared>) -> Json<Option<ShownResult>> {
    let st = state.lock().unwrap();
    Json(st.last_result.clone())
}

// админ -> следующая ситуация
async fn admin_next(State(state): State<Shared>) -> Json<ClickResponse> {
    let mut st = state.lock().unwrap();
    st.current_index = (st.current_index + 1) % st.situations.len();
    st.counts = [0, 0, 0];
    st.last_result = None;
    Json(ClickResponse { ok: true })
}

// админ -> сброс
async fn admin_reset(State(state): State<Shared>) -> Json<ClickResponse> {
    let mut st = state.lock().unwrap();
    st.counts = [0, 0, 0];
    st.last_result = None;
    Json(ClickResponse { ok: true })
}

// ===================== Утилиты =====================

fn idx_to_reaction(i: usize) -> Reaction {
    match i {
        0 => Reaction::Lie,
        1 => Reaction::Delay,
        _ => Reaction::Freeze,
    }
}

fn ordered_tuple(a: Reaction, b: Reaction) -> (Reaction, Reaction) {
    if (a as u8) <= (b as u8) {
        (a, b)
    } else {
        (b, a)
    }
}

fn top_two(counts: &[u64; 3]) -> (Reaction, Reaction) {
    let mut pairs = vec![(counts[0], 0usize), (counts[1], 1usize), (counts[2], 2usize)];
    pairs.sort_by(|a, b| b.0.cmp(&a.0));
    (idx_to_reaction(pairs[0].1), idx_to_reaction(pairs[1].1))
}

// ===================== HTML (клиент) =====================

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8" />
  <title>Корпокликер</title>
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <style>
    :root {
      --bg: #0f172a;
      --panel: rgba(15, 23, 42, 0.45);
      --card: #111827;
      --accent: #38bdf8;
      --text: #e2e8f0;
      --muted: #94a3b8;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background: radial-gradient(circle at top, #0f172a 0, #020617 60%, #020617 100%);
      min-height: 100vh;
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      color: var(--text);
      display: flex;
      justify-content: center;
      padding: 18px;
    }
    .wrap { width: min(600px, 100%); }
    .header {
      display: flex; justify-content: space-between; align-items: center; margin-bottom: 14px;
    }
    .logo { font-weight: 700; display: flex; gap: .5rem; align-items: center; }
    .logo-badge {
      background: rgba(148, 163, 184, .15);
      border: 1px solid rgba(148, 163, 184, .3);
      width: 28px; height: 28px; border-radius: 999px;
      display: grid; place-items: center; font-size: .6rem;
    }
    .status { font-size: .7rem; color: var(--muted); display: flex; gap: .4rem; align-items: center; }
    .dot {
      width: 5px; height: 5px; border-radius: 999px; background: var(--accent);
      animation: pulse 1s ease-in-out infinite;
    }
    @keyframes pulse {
      0% { opacity: .2; transform: scale(1); }
      50% { opacity: 1; transform: scale(1.4); }
      100% { opacity: .2; transform: scale(1); }
    }
    .card {
      background: rgba(2, 6, 23, 0.45);
      border: 1px solid rgba(148, 163, 184, .12);
      border-radius: 18px;
      padding: 16px 16px 10px;
      backdrop-filter: blur(10px);
      margin-bottom: 16px;
    }
    .card h2 { margin: 0 0 6px; font-size: 1.05rem; }
    .card p { margin: 0; color: var(--muted); font-size: .85rem; }
    .buttons { display: grid; gap: 10px; margin-bottom: 8px; }
    .btn {
      background: rgba(15, 23, 42, 0.5);
      border: 1px solid rgba(148, 163, 184, .15);
      border-radius: 14px;
      padding: 10px 14px 10px 12px;
      display: flex; gap: .6rem; align-items: center;
      cursor: pointer;
      transition: transform .06s ease-out, border .06s ease-out, background .06s ease-out;
    }
    .btn:hover { border: 1px solid rgba(148, 163, 184, .4); background: rgba(15, 23, 42, 0.85); }
    .btn:active { transform: scale(.996); }
    .btn-icon {
      width: 32px; height: 32px; border-radius: 12px; display: grid; place-items: center;
      background: rgba(148, 163, 184, .1); font-size: .9rem;
    }
    .btn-label { font-weight: 600; }
    .btn-desc { font-size: .68rem; color: var(--muted); }
    #status { font-size: .72rem; color: #22c55e; min-height: 1.1rem; margin-left: 2px; }
    .answer-box {
      background: rgba(15, 23, 42, 0.3);
      border: 1px solid rgba(148, 163, 184, 0.05);
      border-radius: 12px;
      padding: 9px 11px 10px;
      margin-top: 9px;
      display: none;
    }
    .answer-title {
      font-size: .72rem;
      color: var(--muted);
      margin-bottom: 3px;
      text-transform: uppercase;
      letter-spacing: .03em;
    }
    .answer-text { font-size: .85rem; margin-bottom: 4px; }
    .answer-counts { font-size: .6rem; color: var(--muted); }
    .error {
      color: #f43f5e;
      font-size: .72rem;
      margin-top: 6px;
    }
    @media (min-width: 520px) {
      .buttons { grid-template-columns: repeat(3, minmax(0, 1fr)); }
    }
  </style>
</head>
<body>
  <div class="wrap">
    <div class="header">
      <div class="logo">
        <div class="logo-badge">CF</div>
        Корпокликер
      </div>
    </div>

    <div class="card" id="question-card">
      <h2 id="title">Загрузка…</h2>
      <p id="desc">Получаем ситуацию с сервера</p>
      <p id="error" class="error" style="display:none;"></p>
    </div>

    <div class="buttons">
      <button class="btn" onclick="sendReaction('lie')">
        <div class="btn-icon">🗯</div>
        <div>
          <div class="btn-label">Врать</div>
          <div class="btn-desc">классика корпоративной обороны</div>
        </div>
      </button>
      <button class="btn" onclick="sendReaction('delay')">
        <div class="btn-icon">⏱</div>
        <div>
          <div class="btn-label">Отложить</div>
          <div class="btn-desc">сдвинем на чуть-чуть</div>
        </div>
      </button>
      <button class="btn" onclick="sendReaction('freeze')">
        <div class="btn-icon">🧊</div>
        <div>
          <div class="btn-label">Заморозить тему</div>
          <div class="btn-desc">не сейчас, потом</div>
        </div>
      </button>
    </div>

    <div id="status"></div>

    <div class="answer-box" id="answer-box">
      <div class="answer-title">Коллеги...</div>
      <div class="answer-text" id="answer-text"></div>
      <div class="answer-counts">
        Клики (врать, отложить, заморозить): <span id="answer-counts"></span>
      </div>
    </div>
  </div>

  <script>
    let currentTitle = null;

    async function sendReaction(reaction) {
      await fetch('/api/click', {
        method: 'POST',
        headers: {'Content-Type':'application/json'},
        body: JSON.stringify({reaction})
      });
      document.getElementById('status').innerText = 'Принято, тыкай еще!!!';
    }

    async function pollLoop() {
      try {
        // 1. тянем ситуацию
        const cur = await fetch('/api/current');
        const curData = await cur.json();
        if (curData.title !== currentTitle) {
          currentTitle = curData.title;
          document.getElementById('title').innerText = curData.title;
          document.getElementById('desc').innerText = curData.description;
          // при смене ситуации можно скрыть старый ответ
          document.getElementById('answer-box').style.display = 'none';
        }

        // 2. тянем ответ
        const res = await fetch('/api/result');
        const resData = await res.json();
        const box = document.getElementById('answer-box');
        if (resData) {
          box.style.display = 'block';
          document.getElementById('answer-text').innerText = resData.answer;
          document.getElementById('answer-counts').innerText = resData.counts.join(', ');
        } else {
          // если админ сбросил/переключил
          box.style.display = 'none';
        }

      } catch (e) {
        // можно залогать в консоль
        // console.error(e);
      } finally {
        setTimeout(pollLoop, 1500);
      }
    }

    // старт
    pollLoop();
  </script>
</body>
</html>
"#;

// ===================== HTML (админ) =====================

const ADMIN_HTML: &str = r#"<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8" />
  <title>Админ — Корпокликер</title>
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <style>
    body {
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: #0f172a;
      color: #e2e8f0;
      max-width: 620px;
      margin: 28px auto;
      padding: 0 14px 30px;
    }
    h1 { font-size: 1.1rem; margin-bottom: 10px; }
    .panel {
      background: rgba(15, 23, 42, 0.35);
      border: 1px solid rgba(148, 163, 184, 0.1);
      border-radius: 16px;
      padding: 14px 12px 10px;
      backdrop-filter: blur(10px);
    }
    button {
      background: rgba(15, 23, 42, 0.7);
      border: 1px solid rgba(148, 163, 184, 0.25);
      border-radius: 999px;
      padding: 7px 15px;
      font-size: .8rem;
      color: #e2e8f0;
      cursor: pointer;
      margin-right: 6px;
      margin-bottom: 6px;
      transition: background .08s ease-out;
    }
    button:hover { background: rgba(15, 23, 42, 1); }
    pre {
      white-space: pre-wrap;
      background: rgba(2,6,23,.25);
      border: 1px solid rgba(148,163,184,.05);
      padding: 10px;
      border-radius: 10px;
      margin-top: 10px;
      font-size: .75rem;
    }
  </style>
</head>
<body>
  <h1>Админ — Корпокликер</h1>
  <div class="panel">
    <button onclick="showAnswer()">Показать ответ</button>
    <button onclick="nextSituation()">Дальше</button>
    <button onclick="resetCounts()">Сброс</button>
    <pre id="out">Нажми “Показать ответ”, чтобы отдать его игрокам</pre>
  </div>

  <script>
    async function showAnswer() {
      const r = await fetch('/admin/show');
      const d = await r.json();
      document.getElementById('out').innerText =
        'Ситуация: ' + d.situation_title +
        '\n\nОтвет:\n' + d.answer +
        '\n\nКлики (Врать, Отложить, Заморозить): ' + d.counts.join(', ');
    }
    async function nextSituation() {
      await fetch('/admin/next', {method:'POST'});
      document.getElementById('out').innerText = 'Переключено на следующую ситуацию, клики сброшены.';
    }
    async function resetCounts() {
      await fetch('/admin/reset', {method:'POST'});
      document.getElementById('out').innerText = 'Клики и показанный ответ сброшены.';
    }
  </script>
</body>
</html>
"#;

// ===================== Ситуации =====================

fn build_situations() -> Vec<Situation> {
    let mut v = Vec::new();

    // ===== БЛОК 1: разогревочные =====
    v.push(Situation {
        title: "Почему ретро перенесли?".to_string(),
        description: "Команда интересуется, почему ежедневная встреча испарилась...".to_string(),
        answers: make_answers(
            "Мы хотели, чтобы всем было удобно подключиться, поэтому слегка сдвинули. Чуть позже уточним финальное время.",
            "Там образовался важный созвон, пришлось подвигать. Не закапываемся, идём по текущему расписанию.",
            "Время ещё финалится на уровне выше. Пока работаем так, без обсуждения. (что бы это не значило...)",
        ),
    });

    v.push(Situation {
        title: "Почему в джире опять другие приоритеты?".to_string(),
        description: "Разработчики видят, что задачи снова переприоритизировали...".to_string(),
        answers: make_answers(
            "Это не смена приоритета, мы просто уточнили бизнес-цели. Позже вышлю роадмап.",
            "Так и планировалось, просто вы пока не видите полный контекст. Берём то, что есть.",
            "Это сейчас решается выше. Как скажут - так и возьмём, возвращаться не будем.",
        ),
    });

    v.push(Situation {
        title: "Можно нормальные требования сразу, а не по кусочкам?".to_string(),
        description: "Команда хочет цельное ТЗ...".to_string(),
        answers: make_answers(
            "Они есть, мы их доупаковываем для вас. Как только согласуем - пришлю цельный вариант.",
            "Это практически финальная версия, но у нас вчера был напряжённый релиз, поэтому не всё прошло гладко.",
            "Сейчас не до этого, у нас важный хотфикс. Как стабилизируемся - оформим и вернёмся.",
        ),
    });

    v.push(Situation {
        title: "Зачем ещё один созвон по тому же вопросу?".to_string(),
        description: "Коллеги не рады приглашению на повторную встречу...".to_string(),
        answers: make_answers(
            "Появилась новая информация, надо быстро всех синхронизировать. Детали позже.",
            "Это был изначально контрольный созвон. Просто отметимся и дальше.",
            "Так решили сверху. Проводим и не обсуждаем.",
        ),
    });

    v.push(Situation {
        title: "Почему у нас нет нормальной документации?".to_string(),
        description: "Классическая боль по докам...".to_string(),
        answers: make_answers(
            "Документация ведётся, просто не у всех есть доступ к ней. Уточню, когда выкатят.",
            "Документация есть в рабочем виде. Сейчас это вторично.",
            "Фокус не на этом. Как будут ресурсы - сделаем.",
        ),
    });

    // ===== БЛОК 2: банальные =====
    v.push(Situation {
        title: "Когда будет зарплата за этот месяц?".to_string(),
        description: "Самый ожидаемый вопрос...".to_string(),
        answers: make_answers(
            "Платёж уже ушёл, деньги в пути. Если до конца недели не придут - дёрнем ещё раз.",
            "Она заложена, просто сейчас задержка на стороне бухгалтерии или банка. Не останавливаемся, работаем.",
            "Точной даты сейчас не дадим. Как только будет финал - сообщим единым сообщением.",
        ),
    });

    v.push(Situation {
        title: "Почему нам не сказали заранее про сдвиг выплат?".to_string(),
        description: "Коммуникация зап@зд?ла...".to_string(),
        answers: make_answers(
            "Мы сами узнали в последний момент и не хотели дезинформировать. В следующий раз предупредим раньше.",
            "Информация была, но в рабочем виде. Сейчас не копаемся, идём дальше.",
            "Коммуникацию улучшим. Пока фиксируем, что так случилось.",
        ),
    });

    v.push(Situation {
        title: "Когда нормальный тимбилдинг, а не `пицца дома`?".to_string(),
        description: "И рыбку съесть, и пиццу тоже...".to_string(),
        answers: make_answers(
            "Мы как раз обсуждаем формат, чтобы всем зашло. Чуть позже скинем варианты.",
            "Корпоратив заложен, просто сейчас не время раскрывать детали. Не спойлерим.",
            "Сначала стабилизируемся по выплатам, потом развлечения. Тему пока откладываем.",
        ),
    });

    v.push(Situation {
        title: "А можно нам мерч, чтобы хоть что-то материальное от компании было?".to_string(),
        description: "А зачем мерч если айтышники только дома сидят...".to_string(),
        answers: make_answers(
            "Мерч уже в проработке, ищем подрядчика. Чуть позже соберём размеры.",
            "Мерч - часть HR-стратегии, он не отменён. Сейчас не отвлекаемся от задач.",
            "Пока приоритет не на этом. Вернёмся к мерчу, когда будет окно.",
        ),
    });

    v.push(Situation {
        title: "Почему вы нанимаете людей, если зарплаты задерживаются?".to_string(),
        description: "Про странный приоритет, значитс...".to_string(),
        answers: make_answers(
            "Это разные бюджеты, они не пересекаются. Позже расскажем структуру затрат.",
            "Набор - часть стратегии роста. Не смешиваем это с выплатами.",
            "Этим занимается другой отдел. В общем финобновлении будет ответ.",
        ),
    });

    // ===== БЛОК 3: острые =====
    v.push(Situation {
        title: "Компания вообще жива? Нас не закрывают?".to_string(),
        description: "Панический вопрос!.!".to_string(),
        answers: make_answers(
            "Компания в норме, мы просто в перестройке. Чуть позже покажем все цифры.",
            "У нас всё под контролем, вы видите только часть. Не паникуем, работаем.",
            "Эту тему сейчас не поднимаем. Будет официальный апдейт - получите.",
        ),
    });

    v.push(Situation {
        title: "Почему 'последний раз задержка' уже третий раз?".to_string(),
        description: "Тоторо... Тоторо...".to_string(),
        answers: make_answers(
            "Первые кейсы были внешними. Сейчас выходим на стабильность, позже дам подтверждение.",
            "Мы говорили про те конкретные случаи, этот - другой. Не смешиваем.",
            "Сейчас не копаемся в формулировках. Важно, что двигаемся к нормальному циклу.",
        ),
    });

    v.push(Situation {
        title: "А нас когда уже заменит ИИ, чтобы он получал задержанную зарплату вместо нас?".to_string(),
        description: "Кстати, да...".to_string(),
        answers: make_answers(
            "Мы уже исследуем AI-направление, но людей оно не заменяет. Позже расскажем, как будем использовать.",
            "ИИ - это доп-инструмент, а не замена. Сейчас не уходим в эту тему.",
            "Это не приоритет сейчас. Как будет стратегия по AI - презентуем.",
        ),
    });

    v.push(Situation {
        title: "Почему у Пети MacBook новый, а у меня вентилятор взлетает от гугл-мита?".to_string(),
        description: "У пети просто лицензи на огнестрел есть...".to_string(),
        answers: make_answers(
            "Это был тест рабочего устройства, мы ещё будем раздавать. Чуть позже уточним по технике.",
            "Это под конкретные задачи. Сейчас не будем сравнивать железо.",
            "Сначала закрываем рабочие вопросы. Обновление техники обсудим отдельно.",
        ),
    });

    v.push(Situation {
        title: "Если всё хорошо, почему вы не показываете цифры?".to_string(),
        description: "Вот именно, что цифры...".to_string(),
        answers: make_answers(
            "Мы как раз готовим прозрачный отчёт. Дайте время, чтобы он был корректным.",
            "Цифры положительные, просто они внутренняя инфа. Сейчас не тот формат.",
            "Финансовая инфа будет в официальном канале. Пока тему закрываем.",
        ),
    });

    v
}

fn make_answers(
    lie_delay: &str,
    lie_freeze: &str,
    delay_freeze: &str,
) -> HashMap<(Reaction, Reaction), String> {
    let mut m = HashMap::new();
    m.insert(ordered_tuple(Reaction::Lie, Reaction::Delay), lie_delay.to_string());
    m.insert(ordered_tuple(Reaction::Lie, Reaction::Freeze), lie_freeze.to_string());
    m.insert(ordered_tuple(Reaction::Delay, Reaction::Freeze), delay_freeze.to_string());
    m
}
