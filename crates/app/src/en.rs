//! The interface in English: each Spanish text from the code, and its English
//! version (see `i18n`). Templates keep their `{}`, as many and in the same
//! order as the Spanish ones.
//!
//! Adding a text: write it in Spanish in the code inside `tr("…")`, then add the
//! pair here. The test below fails if a `tr` text has no pair here, if a pair is
//! left without its text in the code, or if the `{}` don't match.

pub const EN: &[(&str, &str)] = &[
    // The menu bar (see `native_menu`), in macOS's title case.
    ("Pulse", "Pulse"),
    ("Archivo", "File"),
    ("Edición", "Edit"),
    ("Crear", "Create"),
    ("Ver", "View"),
    ("Transporte", "Transport"),
    ("Ventana", "Window"),
    ("Ayuda", "Help"),
    ("Acerca de Pulse", "About Pulse"),
    ("Buscar actualizaciones…", "Check for Updates…"),
    ("Ajustes…", "Settings…"),
    ("Ocultar Pulse", "Hide Pulse"),
    ("Ocultar otras", "Hide Others"),
    ("Mostrar todas", "Show All"),
    ("Salir de Pulse", "Quit Pulse"),
    ("Nuevo", "New"),
    ("¿Qué quieres hacer?…", "What Would You Like to Do?…"),
    ("Nuevo desde plantilla", "New from Template"),
    ("Abrir…", "Open…"),
    ("Abrir recientes", "Open Recent"),
    ("Ningún proyecto reciente", "No Recent Projects"),
    ("Borrar la lista", "Clear Menu"),
    ("Guardar", "Save"),
    ("Guardar como…", "Save As…"),
    ("Guardar como plantilla…", "Save as Template…"),
    ("Volver a la versión guardada", "Revert to Saved"),
    ("Versiones…", "Versions…"),
    ("Importar set de Live (.als)…", "Import Live Set (.als)…"),
    ("Importar audio…", "Import Audio…"),
    ("Importar MIDI…", "Import MIDI…"),
    ("Exportar audio…", "Export Audio…"),
    ("Exportar MIDI…", "Export MIDI…"),
    ("Asistente de mezcla y master…", "Mix and Master Assistant…"),
    ("Mostrar la carpeta de Pulse", "Show Pulse Folder"),
    ("Deshacer", "Undo"),
    ("Rehacer", "Redo"),
    ("Cortar", "Cut"),
    ("Copiar", "Copy"),
    ("Pegar", "Paste"),
    ("Duplicar", "Duplicate"),
    ("Dividir clip", "Split Clip"),
    ("Unir clips", "Join Clips"),
    ("Eliminar  (⌫)", "Delete  (⌫)"),
    ("Seleccionar todas las notas", "Select All Notes"),
    ("Pista MIDI", "MIDI Track"),
    ("Pista de audio", "Audio Track"),
    ("Retorno", "Return Track"),
    ("Duplicar pista", "Duplicate Track"),
    ("Eliminar pista", "Delete Track"),
    ("Agrupar pistas", "Group Tracks"),
    ("Desagrupar", "Ungroup"),
    ("Arreglo automático…", "Auto-Arrange…"),
    ("Session  (Tab)", "Session  (Tab)"),
    ("Arrangement  (Tab)", "Arrangement  (Tab)"),
    ("Clip o dispositivos  (⇧Tab)", "Clip or Devices  (⇧Tab)"),
    ("Maximizar el editor", "Maximize Editor"),
    ("Mostrar u ocultar el navegador", "Show or Hide Browser"),
    ("Pantalla completa", "Full Screen"),
    ("Reproducir / Parar  (Espacio)", "Play / Stop  (Space)"),
    ("Grabar en el Arrangement  (F9)", "Record to Arrangement  (F9)"),
    ("Capturar MIDI", "Capture MIDI"),
    ("Loop", "Loop"),
    ("Metrónomo", "Metronome"),
    ("Armar automatización", "Automation Arm"),
    ("Teclado del ordenador  (M)", "Computer Keyboard  (M)"),
    ("Asignar controles MIDI", "Assign MIDI Controls"),
    ("Macros…", "Macros…"),
    ("Minimizar", "Minimize"),
    ("Zoom", "Zoom"),
    ("Guía de Pulse", "Pulse Guide"),
    ("Lecciones", "Lessons"),
    ("Atajos de teclado", "Keyboard Shortcuts"),
    // Key names off macOS (see `theme::keys`).
    ("Supr", "Del"),
    ("Mayús", "Shift"),
    // Ajustes (see `settings`).
    ("Ajustes", "Settings"),
    ("Como el sistema ({})", "Same as the system ({})"),
    ("La traducción al inglés está en marcha: lo que aún no está traducido sale en castellano.", "The English translation is in progress: what isn't translated yet shows in Spanish."),
    ("En uso: {}", "In use: {}"),
    ("Audio: {}", "Audio: {}"),
    ("búfer por defecto", "default buffer"),
    ("{} muestras ({} ms)", "{} samples ({} ms)"),
    ("Predeterminado del sistema", "System default"),
    ("Salida", "Output"),
    ("Entrada (grabar audio)", "Input (audio recording)"),
    ("Frecuencia de muestreo", "Sample rate"),
    ("Predeterminada", "Default"),
    ("Tamaño del búfer", "Buffer size"),
    ("Predeterminado", "Default"),
    ("{} muestras · {} ms", "{} samples · {} ms"),
    ("Un búfer menor responde antes al tocar; si oyes cortes, súbelo.", "A smaller buffer responds sooner when you play; if you hear dropouts, raise it."),
    ("Latencia: entrada {} · salida {} ms{}. Pulse la descuenta al grabar para que las tomas caigan a tempo.", "Latency: input {} · output {} ms{}. Pulse subtracts it when recording, so takes land in time."),
    ("— (sin abrir)", "— (not open)"),
    (" · efectos {} ms", " · effects {} ms"),
    ("Compensar la latencia de los efectos", "Compensate for effect latency"),
    ("Algunos efectos sacan el sonido un poco tarde: el Limiter (1,5 ms), el Saturator con sobremuestreo, el Tuner y muchos plugins. Con esto, las demás pistas los esperan y todo suena a la vez, y el metrónomo con ellas. Quítalo para tocar o cantar en directo a través de un efecto así sin que el resto se retrase", "Some effects put their sound out slightly late: the Limiter (1.5 ms), the Saturator with oversampling, the Tuner and many plugins. With this on, the other tracks wait for them and everything sounds together, the metronome too. Turn it off to play or sing live through such an effect without the rest being delayed"),
    ("Ajuste fino de la grabación", "Recording offset"),
    ("Si las tomas de audio quedan tarde, súbelo (las adelanta); si quedan pronto, bájalo", "If audio takes land late, raise it (it moves them earlier); if they land early, lower it"),
    ("Volver a buscar dispositivos", "Look for devices again"),
    ("Ningún dispositivo MIDI conectado", "No MIDI devices connected"),
    ("Volver a buscar dispositivos MIDI", "Look for MIDI devices again"),
    ("Quitar", "Remove"),
    ("Añadir carpeta…", "Add folder…"),
    ("Buscar plugins", "Find plugins"),
    ("{} plugins encontrados", "{} plugins found"),
    ("Bloqueados: no se cargan (cerraron Pulse). Sus pistas quedan en silencio, con sus ajustes guardados", "Blocked: they don't load (they closed Pulse). Their tracks stay silent, with their settings kept"),
    ("Desbloquear", "Unblock"),
    ("Vuelve a cargarse en los proyectos que lo usan. Si cierra Pulse otra vez, Pulse te lo volverá a preguntar", "It loads again in the projects that use it. If it closes Pulse again, Pulse will ask you again"),
    ("Carpeta de Pulse", "Pulse folder"),
    ("Proyectos (cada uno en su carpeta, con sus audios), exportaciones, presets, wavetables y muestras", "Projects (each in its own folder, with its audio), exports, presets, wavetables and samples"),
    ("Cambiar…", "Change…"),
    ("Lo que ya hay no se mueve: los proyectos se abren desde donde estén", "What's already there doesn't move: projects open from wherever they are"),
    ("Volver a Música/Pulse", "Back to Music/Pulse"),
    ("Mostrar en el Explorador", "Show in Explorer"),
    ("Mostrar en Finder", "Show in Finder"),
    ("Proyectos", "Projects"),
    ("Abrir el último proyecto al arrancar", "Open the last project at startup"),
    ("Mostrar «¿Qué quieres hacer?» al arrancar sin proyecto", "Show \"What would you like to do?\" when starting without a project"),
    ("La ventana con la canción de ejemplo, las plantillas, las lecciones y los recientes. Siempre está en Archivo › ¿Qué quieres hacer?…", "The window with the example song, the templates, the lessons and recent projects. It's always in File › What Would You Like to Do?…"),
    ("Proyecto nuevo (⌘N)", "New project (⌘N)"),
    ("Clips MIDI nuevos", "New MIDI clips"),
    ("1 compás", "1 bar"),
    ("{} compases", "{} bars"),
    ("Lo que mide un clip MIDI al crearlo con doble clic (en la Session o en el Arrangement) o con «Nuevo clip MIDI aquí». Luego cambias el de cada uno en «Loop», en el editor", "How long a MIDI clip is when you create it by double-clicking (in the Session or the Arrangement) or with \"New MIDI clip here\". You then change each one's in \"Loop\", in the editor"),
    ("Buscar actualizaciones al arrancar", "Check for updates at startup"),
    // Grid controllers, in Ajustes (see `grid_io`).
    ("Controladores de rejilla", "Grid controllers"),
    ("Ninguno conectado. Pulse reconoce solo los Launchpad Mini MK3, X y Pro MK3, los APC mini (y mk2) y el APC40 mkII; otro con pads, apréndelo abajo.", "None connected. Pulse only recognises the Launchpad Mini MK3, X and Pro MK3, the APC mini (and mk2) and the APC40 mkII; for another one with pads, teach it below."),
    ("pistas {}–{} · escenas {}–{}", "tracks {}–{} · scenes {}–{}"),
    ("Mueve la zona que controla (el recuadro de la Session)", "Moves the area it controls (the box in the Session)"),
    ("Cancelar", "Cancel"),
    ("Olvidar la rejilla", "Forget the grid"),
    ("Sus notas vuelven a tocar los instrumentos", "Its notes play the instruments again"),
    ("Aprender como rejilla", "Learn as a grid"),
    ("Pulsa tres pads: el de arriba a la izquierda, el de su derecha y el de debajo. Desde entonces sus pads lanzan clips y no tocan notas", "Press three pads: the top-left one, the one to its right and the one below it. From then on its pads launch clips and don't play notes"),
    // «¿Qué quieres hacer?» (see `welcome`).
    ("¿Qué quieres hacer?", "What would you like to do?"),
    ("Empieza por donde quieras: todo se puede cambiar después.", "Start wherever you like: everything can be changed later."),
    ("Escuchar el ejemplo", "Listen to the example"),
    ("«{}»: un tema corto con escenas para lanzar y la canción montada, para oír y desmontar", "\"{}\": a short track with scenes to launch and the arranged song, to listen to and take apart"),
    ("Tararear una idea", "Hum an idea"),
    ("Graba tu melodía y Pulse la convierte en canción: melodía, acordes, bajo y batería", "Record your melody and Pulse turns it into a song: melody, chords, bass and drums"),
    ("Remezclar una canción", "Remix a song"),
    ("Elige un audio y Pulse lo separa en batería, bajo, voz y el resto, cada uno en su pista", "Choose an audio file and Pulse splits it into drums, bass, vocals and the rest, each on its own track"),
    ("EMPEZAR DE UNA PLANTILLA", "START FROM A TEMPLATE"),
    ("Dos pistas MIDI y nada más", "Two MIDI tracks and nothing else"),
    ("Una plantilla tuya", "One of your templates"),
    ("APRENDER", "LEARN"),
    ("ABRIR", "OPEN"),
    ("Aún no hay proyectos recientes", "No recent projects yet"),
    ("Abrir un proyecto…", "Open a project…"),
    ("Mostrar al arrancar", "Show at startup"),
    ("Si no, Pulse empieza con la canción de ejemplo o el último proyecto. Siempre está en Archivo › ¿Qué quieres hacer?…", "If not, Pulse starts with the example song or the last project. It's always in File › What Would You Like to Do?…"),
    ("Cerrar", "Close"),
    ("Abrir la guía", "Open the guide"),
    ("Primeros pasos, en la guía de Pulse", "Getting started, in the Pulse guide (in Spanish)"),
    ("Canción de ejemplo «{}»: Espacio para oírla; en la Session (Tab) lanza sus escenas", "Example song \"{}\": Space to hear it; in the Session (Tab), launch its scenes"),
    ("No se pudo preparar la canción de ejemplo: {}", "Couldn't prepare the example song: {}"),
    ("Pista «Tarareo» armada: F9 graba, Espacio para", "Track \"Tarareo\" armed: F9 records, Space stops"),
    // The `i18n` tests.
    ("una frase que no está en la tabla", "una frase que no está en la tabla"),
];

#[cfg(test)]
mod tests {
    use super::EN;
    use std::collections::BTreeSet;

    /// The Spanish texts the code shows through `tr`: `tr("…")` literals, and
    /// the menu bar's titles (kept in `native_menu::MENUS`).
    fn used() -> BTreeSet<String> {
        let mut used = BTreeSet::new();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.file_name().is_some_and(|n| n == "en.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let mut rest = source.as_str();
            while let Some(at) = rest.find("tr(") {
                let before = rest[..at].chars().last();
                rest = &rest[at + 3..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let text = rest.trim_start();
                if let Some(literal) = text.strip_prefix('"') {
                    used.insert(unescape(literal));
                }
            }
        }
        for (title, items) in crate::native_menu::MENUS {
            used.insert(title.to_string());
            for item in items {
                match item {
                    crate::native_menu::Item::Run(_, label, _) | crate::native_menu::Item::System(label, ..) => {
                        used.insert(label.to_string());
                    }
                    _ => {}
                }
            }
        }
        used
    }

    /// A Rust string literal's text up to its closing quote.
    fn unescape(literal: &str) -> String {
        let mut out = String::new();
        let mut chars = literal.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => break,
                },
                c => out.push(c),
            }
        }
        out
    }

    #[test]
    fn every_text_has_its_english_and_every_english_its_text() {
        let used = used();
        let table: BTreeSet<String> = EN.iter().map(|(es, _)| es.to_string()).collect();
        let missing: Vec<&String> = used.difference(&table).collect();
        assert!(missing.is_empty(), "sin inglés en en.rs: {missing:#?}");
        let unused: Vec<&String> = table.difference(&used).collect();
        assert!(unused.is_empty(), "en en.rs pero ya no en el código: {unused:#?}");
        assert_eq!(table.len(), EN.len(), "a Spanish text twice in en.rs");
    }

    #[test]
    fn templates_keep_their_gaps() {
        for (es, en) in EN {
            assert_eq!(es.matches("{}").count(), en.matches("{}").count(), "«{es}» → «{en}»");
            assert!(!en.is_empty(), "«{es}»");
        }
    }
}
