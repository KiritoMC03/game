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
  <title>Корпоративный рандомайзер</title>
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <style>
    body { font-family: sans-serif; max-width: 560px; margin: 40px auto; }
    button { margin: 6px 0; padding: 10px 14px; font-size: 15px; width: 100%; cursor: pointer; }
    .box { border: 1px solid #ddd; padding: 16px; border-radius: 8px; margin-bottom: 14px; }
    #status { color: #4e7; }
    #answer-box { background: #f4f4f4; padding: 12px; border-radius: 8px; display: none; }
  </style>
</head>
<body>
  <h1>Корпоративный рандомайзер</h1>
  <div class="box">
    <h2 id="title">Загрузка…</h2>
    <p id="desc"></p>
  </div>
  <div>
    <button onclick="sendReaction('lie')">Врать</button>
    <button onclick="sendReaction('delay')">Отложить</button>
    <button onclick="sendReaction('freeze')">Заморозить тему</button>
  </div>
  <p id="status"></p>

  <div id="answer-box">
    <h3>Ответ ведущего</h3>
    <p id="answer-text"></p>
    <p><b>Клики:</b> <span id="answer-counts"></span></p>
  </div>

  <script>
    let currentTitle = null;

    async function sendReaction(reaction) {
      await fetch('/api/click', {
        method: 'POST',
        headers: {'Content-Type':'application/json'},
        body: JSON.stringify({reaction})
      });
      document.getElementById('status').innerText = 'Принято 👍';
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
  <title>Админ</title>
  <style>
    body { font-family: sans-serif; max-width: 560px; margin: 40px auto; }
    button { margin: 6px 0; padding: 10px 14px; font-size: 15px; cursor: pointer; }
    pre { white-space: pre-wrap; background: #f7f7f7; padding: 10px; border-radius: 4px; }
  </style>
</head>
<body>
  <h1>Админка</h1>
  <button onclick="showAnswer()">Показать ответ</button>
  <button onclick="nextSituation()">Дальше</button>
  <button onclick="resetCounts()">Сброс</button>
  <pre id="out"></pre>
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
        title: "Почему стендап опять перенесли?".to_string(),
        description: "Команда интересуется, почему ежедневная встреча снова уехала по времени.".to_string(),
        answers: make_answers(
            "Мы хотели, чтобы всем было удобно подключиться, поэтому слегка сдвинули. Чуть позже уточним финальное время.",
            "Там накрылся важный созвон, пришлось подвигать. Не закапываемся, идём по текущему расписанию.",
            "Время ещё финалится на уровне выше. Пока работаем так, без обсуждения.",
        ),
    });

    v.push(Situation {
        title: "Почему в таск-трекере опять другие приоритеты?".to_string(),
        description: "Разработчики видят, что задачи снова переприоритизировали.".to_string(),
        answers: make_answers(
            "Это не смена приоритета, мы просто уточнили бизнес-цели. Позже вышлю расклад.",
            "Так и планировалось, просто вы пока не видите полный контекст. Берём то, что есть.",
            "Это сейчас решается выше. Как скажут — так и возьмём, возвращаться не будем.",
        ),
    });

    v.push(Situation {
        title: "Можно нормальные требования сразу, а не по кусочкам?".to_string(),
        description: "Команда хочет цельное ТЗ.".to_string(),
        answers: make_answers(
            "Они есть, мы их доупаковываем для вас. Как только согласуем — пришлю цельный вариант.",
            "Это и есть финальная версия, она просто живая. Не распаковываем сейчас.",
            "Сейчас не до этого. Как стабилизируемся — оформим и вернёмся.",
        ),
    });

    v.push(Situation {
        title: "Зачем ещё один созвон по тому же вопросу?".to_string(),
        description: "Снова приглашение на повтор встречи.".to_string(),
        answers: make_answers(
            "Появилась новая информация, надо быстро всех синхронизировать. Детали позже.",
            "Это был изначально контрольный созвон. Просто отметимся и дальше.",
            "Так решили сверху. Проводим и не обсуждаем.",
        ),
    });

    v.push(Situation {
        title: "Почему у нас нет нормальной документации?".to_string(),
        description: "Классическая боль по докам.".to_string(),
        answers: make_answers(
            "Документация ведётся, просто вы её пока не видите. Уточню, когда выкатят.",
            "Документация есть в рабочем виде. Сейчас это вторично.",
            "Фокус не на этом. Как будут ресурсы — сделаем.",
        ),
    });

    // ===== БЛОК 2: банальные =====
    v.push(Situation {
        title: "Когда будет зарплата за этот месяц?".to_string(),
        description: "Самый ожидаемый вопрос.".to_string(),
        answers: make_answers(
            "Платёж уже ушёл, деньги в пути. Если до конца недели не придут — дёрнем ещё раз.",
            "Она заложена, просто сейчас задержка на стороне бухгалтерии/банка. Не останавливаемся.",
            "Точной даты сейчас не дадим. Как только будет финал — сообщим единым сообщением.",
        ),
    });

    v.push(Situation {
        title: "Вы говорили, что задержек больше не будет. Что случилось?".to_string(),
        description: "Вопрос про доверие к обещаниям.".to_string(),
        answers: make_answers(
            "Это не задержка, а разовый сдвиг из-за перераспределения средств. Чуть позже дам детали.",
            "Мы придерживаемся прежнего курса, просто это форс-мажорный кейс. Не смешиваем.",
            "Тема у финблока. Вернёмся уже с готовым комментарием.",
        ),
    });

    v.push(Situation {
        title: "Будет ли индексация или премии в этом квартале?".to_string(),
        description: "Вопрос про мотивацию.".to_string(),
        answers: make_answers(
            "Это в плане, ничего не отменяли. После сверки бюджета дадим конкретику.",
            "Мотивация никуда не делась, важно сейчас довести спринт — и всё подтянется.",
            "Пока приоритет — стабильность выплат. К плюшкам вернёмся позже.",
        ),
    });

    v.push(Situation {
        title: "Почему нам не сказали заранее про сдвиг выплат?".to_string(),
        description: "Коммуникация запоздала.".to_string(),
        answers: make_answers(
            "Мы сами узнали в последний момент и не хотели дезинформировать. В следующий раз предупредим раньше.",
            "Информация была, но в рабочем виде. Сейчас не копаемся, идём дальше.",
            "Коммуникацию улучшим. Пока фиксируем, что так случилось.",
        ),
    });

    v.push(Situation {
        title: "Почему вы нанимаете людей, если зарплаты задерживаются?".to_string(),
        description: "Про странный приоритет.".to_string(),
        answers: make_answers(
            "Это разные бюджеты, они не пересекаются. Позже расскажем структуру затрат.",
            "Набор — часть стратегии роста. Не смешиваем это с выплатами.",
            "Этим занимается другой отдел. В общем финобновлении будет ответ.",
        ),
    });

    // ===== БЛОК 3: острые =====
    v.push(Situation {
        title: "Компания вообще жива? Нас не закрывают?".to_string(),
        description: "Панический вопрос.".to_string(),
        answers: make_answers(
            "Компания в норме, мы просто в перестройке. Чуть позже покажу цифры.",
            "У нас всё под контролем, вы видите только часть. Не паникуем, работаем.",
            "Эту тему сейчас не поднимаем. Будет официальный апдейт — получите.",
        ),
    });

    v.push(Situation {
        title: "Почему у руководства всё ок, а у нас 'сдвиг выплат'?".to_string(),
        description: "Про справедливость.".to_string(),
        answers: make_answers(
            "Там фиксированные обязательства, их нельзя двигать. По команде тоже выровняем, но позже.",
            "Все в одинаковых условиях, просто формат разный. Не раздуваем.",
            "Сейчас не сравниваем. На финсозвоне объяснят.",
        ),
    });

    v.push(Situation {
        title: "Почему 'последний раз задержка' уже третий раз?".to_string(),
        description: "Про повторяющиеся обещания.".to_string(),
        answers: make_answers(
            "Первые кейсы были внешними. Сейчас выходим на стабильность, позже дам подтверждение.",
            "Мы говорили про те конкретные случаи, этот — другой. Не смешиваем.",
            "Сейчас не копаемся в формулировках. Важно, что двигаемся к нормальному циклу.",
        ),
    });

    v.push(Situation {
        title: "Если всё хорошо, почему вы не показываете цифры?".to_string(),
        description: "Про прозрачность.".to_string(),
        answers: make_answers(
            "Мы как раз готовим прозрачный отчёт. Дайте время, чтобы он был корректным.",
            "Цифры положительные, просто они внутренняя инфа. Сейчас не тот формат.",
            "Финансовая инфа будет в официальном канале. Пока тему закрываем.",
        ),
    });

    v.push(Situation {
        title: "Когда всё это закончится и мы будем получать вовремя?".to_string(),
        description: "Финальный, самый жизненный.".to_string(),
        answers: make_answers(
            "Мы уже на финишной прямой, остались техмоменты. Чуть позже обозначим даты.",
            "Процесс уже выстроен, просто вы пока не чувствуете результат. Не нагнетаем.",
            "Как только стабилизируем кассовые разрывы — так сразу. До этого к теме не возвращаемся.",
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
