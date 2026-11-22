//! Strings for Labs › Watch and learn and its Skills pane, kept in their own
//! table so the French, Portuguese, and Russian tables stay under the source
//! cap. The card's Polish, Chinese, Japanese, German, Spanish, and Ukrainian
//! rows live with the other lab cards in `labs.rs`; the pane's go here.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Ollama isn't running", ["Ollama nie jest uruchomiona", "Ollama 未运行", "Ollama が起動していません", "Ollama läuft nicht", "Ollama no se está ejecutando", "Ollama не запущено"]),
    ("{} isn't downloaded", ["{} nie jest pobrany", "{} 尚未下载", "{} はダウンロードされていません", "{} ist nicht heruntergeladen", "{} no está descargado", "{} не завантажено"]),
    ("Ollama's address must be on this computer", ["Adres Ollamy musi wskazywać ten komputer", "Ollama 的地址必须在这台电脑上", "Ollama のアドレスはこのコンピュータ上にある必要があります", "Die Adresse von Ollama muss auf diesem Computer liegen", "La dirección de Ollama debe estar en este ordenador", "Адреса Ollama має бути на цьому комп'ютері"]),
    ("The model refused this lesson", ["Model odrzucił tę lekcję", "模型拒绝了这节课", "モデルがこのレッスンを拒否しました", "Das Modell hat diese Lektion abgelehnt", "El modelo rechazó esta lección", "Модель відхилила цей урок"]),
    ("The model took too long to answer", ["Model zbyt długo nie odpowiadał", "模型响应时间过长", "モデルの応答に時間がかかりすぎました", "Das Modell hat zu lange für die Antwort gebraucht", "El modelo tardó demasiado en responder", "Модель відповідала надто довго"]),
    ("The model failed: {}", ["Błąd modelu: {}", "模型出错：{}", "モデルでエラーが発生しました: {}", "Das Modell ist fehlgeschlagen: {}", "El modelo falló: {}", "Помилка моделі: {}"]),
    ("Watch and learn is off", ["Funkcja Obserwuj i ucz się jest wyłączona", "观察并学习已关闭", "「見て学ぶ」はオフです", "Zuschauen und lernen ist aus", "Observar y aprender está desactivado", "Функцію «Спостерігай і вчись» вимкнено"]),
    ("Apprentice couldn't start drafting", ["Apprentice nie mógł rozpocząć tworzenia szkicu", "Apprentice 无法开始起草", "Apprentice は下書きを開始できませんでした", "Apprentice konnte den Entwurf nicht beginnen", "Apprentice no pudo empezar el borrador", "Apprentice не вдалося почати чернетку"]),
    ("This lesson has nothing to learn from", ["W tej lekcji nie ma niczego, z czego można się uczyć", "这节课中没有可学习的内容", "このレッスンには学べる内容がありません", "Aus dieser Lektion lässt sich nichts lernen", "No hay nada que aprender de esta lección", "У цьому уроці немає з чого вчитися"]),
    ("Drafting took too long", ["Tworzenie szkicu trwało zbyt długo", "起草耗时过长", "下書きの作成に時間がかかりすぎました", "Der Entwurf hat zu lange gedauert", "El borrador tardó demasiado", "Створення чернетки тривало надто довго"]),
    ("This part of the lesson is too long for Apple Intelligence", ["Ta część lekcji jest zbyt długa dla Apple Intelligence", "这部分课程对 Apple Intelligence 来说太长", "レッスンのこの部分は Apple Intelligence には長すぎます", "Dieser Teil der Lektion ist zu lang für Apple Intelligence", "Esta parte de la lección es demasiado larga para Apple Intelligence", "Ця частина уроку задовга для Apple Intelligence"]),
    ("Sees images · recommended", ["Widzi obrazy · zalecany", "可识别图像 · 推荐", "画像を認識 · おすすめ", "Erkennt Bilder · empfohlen", "Ve imágenes · recomendado", "Бачить зображення · рекомендовано"]),
    ("Sees images · strongest that fits", ["Widzi obrazy · najmocniejszy, który się mieści", "可识别图像 · 可容纳的最强模型", "画像を認識 · 収まる中で最も高性能", "Erkennt Bilder · das stärkste, das passt", "Ve imágenes · el más potente que cabe", "Бачить зображення · найпотужніша, що вміщається"]),
    ("Sees images · lighter", ["Widzi obrazy · lżejszy", "可识别图像 · 更轻量", "画像を認識 · 軽め", "Erkennt Bilder · leichter", "Ve imágenes · más ligero", "Бачить зображення · легша"]),
    ("Sees images · lightest", ["Widzi obrazy · najlżejszy", "可识别图像 · 最轻量", "画像を認識 · 最軽量", "Erkennt Bilder · am leichtesten", "Ve imágenes · el más ligero", "Бачить зображення · найлегша"]),
];

/// Brazilian Portuguese, for the card and the pane.
#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Watch and learn", "Observar e aprender"),
    ("Show Apprentice a task once and it writes a skill your coding agents can follow", "Mostre uma tarefa ao Apprentice uma vez e ele escreve uma habilidade que seus agentes de código podem seguir"),
    ("Ollama isn't running", "O Ollama não está em execução"),
    ("{} isn't downloaded", "{} não foi baixado"),
    ("Ollama's address must be on this computer", "O endereço do Ollama deve estar neste computador"),
    ("The model refused this lesson", "O modelo recusou esta lição"),
    ("The model took too long to answer", "O modelo demorou demais para responder"),
    ("The model failed: {}", "O modelo falhou: {}"),
    ("Watch and learn is off", "Observar e aprender está desativado"),
    ("Apprentice couldn't start drafting", "O Apprentice não conseguiu começar o rascunho"),
    ("This lesson has nothing to learn from", "Não há nada para aprender nesta lição"),
    ("Drafting took too long", "O rascunho demorou demais"),
    ("This part of the lesson is too long for Apple Intelligence", "Esta parte da lição é longa demais para o Apple Intelligence"),
    ("Sees images · recommended", "Vê imagens · recomendado"),
    ("Sees images · strongest that fits", "Vê imagens · o mais forte que cabe"),
    ("Sees images · lighter", "Vê imagens · mais leve"),
    ("Sees images · lightest", "Vê imagens · o mais leve"),
];

/// French, for the card and the pane.
#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Watch and learn", "Observer et apprendre"),
    ("Show Apprentice a task once and it writes a skill your coding agents can follow", "Montrez une tâche à Apprentice une fois et il écrit une compétence que vos agents de code peuvent suivre"),
    ("Ollama isn't running", "Ollama n'est pas lancé"),
    ("{} isn't downloaded", "{} n'est pas téléchargé"),
    ("Ollama's address must be on this computer", "L'adresse d'Ollama doit être sur cet ordinateur"),
    ("The model refused this lesson", "Le modèle a refusé cette leçon"),
    ("The model took too long to answer", "Le modèle a mis trop de temps à répondre"),
    ("The model failed: {}", "Le modèle a échoué : {}"),
    ("Watch and learn is off", "Observer et apprendre est désactivé"),
    ("Apprentice couldn't start drafting", "Apprentice n'a pas pu commencer le brouillon"),
    ("This lesson has nothing to learn from", "Cette leçon ne contient rien dont apprendre"),
    ("Drafting took too long", "Le brouillon a pris trop de temps"),
    ("This part of the lesson is too long for Apple Intelligence", "Cette partie de la leçon est trop longue pour Apple Intelligence"),
    ("Sees images · recommended", "Voit les images · recommandé"),
    ("Sees images · strongest that fits", "Voit les images · le plus puissant qui tient"),
    ("Sees images · lighter", "Voit les images · plus léger"),
    ("Sees images · lightest", "Voit les images · le plus léger"),
];

/// Russian, for the card and the pane.
#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Watch and learn", "Смотреть и учиться"),
    ("Show Apprentice a task once and it writes a skill your coding agents can follow", "Покажите Apprentice задачу один раз, и он напишет навык, которому смогут следовать ваши агенты для кода"),
    ("Ollama isn't running", "Ollama не запущена"),
    ("{} isn't downloaded", "{} не загружена"),
    ("Ollama's address must be on this computer", "Адрес Ollama должен быть на этом компьютере"),
    ("The model refused this lesson", "Модель отклонила этот урок"),
    ("The model took too long to answer", "Модель отвечала слишком долго"),
    ("The model failed: {}", "Ошибка модели: {}"),
    ("Watch and learn is off", "«Смотреть и учиться» выключено"),
    ("Apprentice couldn't start drafting", "Apprentice не удалось начать черновик"),
    ("This lesson has nothing to learn from", "В этом уроке не из чего учиться"),
    ("Drafting took too long", "Создание черновика заняло слишком много времени"),
    ("This part of the lesson is too long for Apple Intelligence", "Эта часть урока слишком длинная для Apple Intelligence"),
    ("Sees images · recommended", "Видит изображения · рекомендуется"),
    ("Sees images · strongest that fits", "Видит изображения · самая мощная, что помещается"),
    ("Sees images · lighter", "Видит изображения · полегче"),
    ("Sees images · lightest", "Видит изображения · самая лёгкая"),
];
