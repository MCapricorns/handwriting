use tracing::{info, warn};
use windows::Foundation::Point;
use windows::Foundation::Size;
use windows::UI::Input::Inking::{
    InkDrawingAttributes, InkRecognitionTarget, InkRecognizerContainer, InkStrokeBuilder,
    InkStrokeContainer, PenTipShape,
};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows_collections::IIterable;

#[derive(Clone, Copy, Debug)]
pub struct InkPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug)]
pub struct RecognitionCandidate {
    pub text: String,
}

pub struct InkRecognizerService {
    container: InkRecognizerContainer,
    recognizer_name: String,
}

impl InkRecognizerService {
    pub fn new() -> windows::core::Result<Self> {
        let container = InkRecognizerContainer::new()?;
        let recognizer_name = select_chinese_recognizer(&container)?;
        Ok(Self {
            container,
            recognizer_name,
        })
    }

    pub fn recognize(
        &self,
        strokes: &[Vec<InkPoint>],
    ) -> windows::core::Result<Vec<RecognitionCandidate>> {
        if strokes.is_empty() {
            return Ok(Vec::new());
        }

        let stroke_container = InkStrokeContainer::new()?;
        let builder = InkStrokeBuilder::new()?;
        let attributes = InkDrawingAttributes::new()?;
        attributes.SetColor(windows::UI::Color {
            A: 255,
            R: 0,
            G: 0,
            B: 0,
        })?;
        attributes.SetSize(Size {
            Width: 3.0,
            Height: 3.0,
        })?;
        attributes.SetIgnorePressure(false)?;
        attributes.SetFitToCurve(true)?;
        attributes.SetPenTip(PenTipShape::Circle)?;
        builder.SetDefaultDrawingAttributes(&attributes)?;

        for stroke_points in strokes {
            if stroke_points.len() < 2 {
                continue;
            }
            let points: Vec<Point> = stroke_points
                .iter()
                .map(|p| Point { X: p.x, Y: p.y })
                .collect();
            let iterable: IIterable<Point> = points.into();
            let stroke = builder.CreateStroke(&iterable)?;
            stroke_container.AddStroke(&stroke)?;
        }

        info!(recognizer = %self.recognizer_name, "running Windows Ink recognition");

        let operation = self
            .container
            .RecognizeAsync(&stroke_container, InkRecognitionTarget::All)?;
        let results = operation.get()?;

        let mut candidates = Vec::new();
        let count = results.Size()?;
        for i in 0..count {
            let result = results.GetAt(i)?;
            let text_candidates = result.GetTextCandidates()?;
            let candidate_count = text_candidates.Size()?;
            for j in 0..candidate_count {
                let text = text_candidates.GetAt(j)?.to_string();
                if text.is_empty() {
                    continue;
                }
                candidates.push(RecognitionCandidate { text });
            }
        }

        if let Some(best) = candidates.first() {
            info!(text = %best.text, "top recognition candidate");
        }

        Ok(candidates)
    }
}

pub fn recognize_off_thread(
    strokes: Vec<Vec<InkPoint>>,
) -> windows::core::Result<Vec<RecognitionCandidate>> {
    std::thread::spawn(
        move || -> windows::core::Result<Vec<RecognitionCandidate>> {
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
            }
            let service = InkRecognizerService::new()?;
            service.recognize(&strokes)
        },
    )
    .join()
    .map_err(|_| windows::core::Error::from(windows::core::HRESULT(-1)))?
}

fn select_chinese_recognizer(container: &InkRecognizerContainer) -> windows::core::Result<String> {
    let recognizers = container.GetRecognizers()?;
    let count = recognizers.Size()?;

    let mut fallback_name = String::new();
    for i in 0..count {
        let recognizer = recognizers.GetAt(i)?;
        let name = recognizer.Name()?.to_string();
        if fallback_name.is_empty() {
            fallback_name = name.clone();
        }
        info!(name = %name, "available ink recognizer");
        if is_chinese_recognizer(&name) {
            container.SetDefaultRecognizer(&recognizer)?;
            info!(name = %name, "selected Chinese ink recognizer");
            return Ok(name);
        }
    }

    if !fallback_name.is_empty() {
        warn!(
            name = %fallback_name,
            "no Chinese ink recognizer found; install Chinese handwriting language pack in Settings > Time & Language > Language"
        );
        return Ok(fallback_name);
    }

    Err(windows::core::Error::from(windows::core::HRESULT(-1)))
}

fn is_chinese_recognizer(name: &str) -> bool {
    let lower = name.to_lowercase();
    name.contains("中文")
        || name.contains("简体")
        || name.contains("繁體")
        || lower.contains("chinese")
        || lower.contains("zh-cn")
        || lower.contains("zh-hans")
        || lower.contains("zh-hant")
        || lower.contains("zh-tw")
        || lower.contains("(china)")
        || lower.contains("(taiwan)")
}
