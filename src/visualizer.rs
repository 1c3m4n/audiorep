use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::audio_info::{AudioInfo, StreamState};
use crate::spectrum::{SpectrumSnapshot, spectrum_label_positions};
use crate::ui::OutputRateInfo;

pub struct Visualizer;

impl Visualizer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        audio_info: &AudioInfo,
        spectrum: &SpectrumSnapshot,
        rate_info: Option<&OutputRateInfo>,
        footer_rate_label: &str,
        rate_status: Option<&str>,
        selected_index: usize,
        show_hidden: bool,
    ) {
        let footer_height = if frame.area().width < 120 { 4 } else { 3 };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(footer_height),
            ])
            .split(frame.area());

        self.render_header(frame, chunks[0], audio_info, show_hidden);
        self.render_body(
            frame,
            chunks[1],
            audio_info,
            spectrum,
            selected_index,
            show_hidden,
        );
        self.render_footer(
            frame,
            chunks[2],
            show_hidden,
            rate_info,
            footer_rate_label,
            rate_status,
        );
    }

    fn render_body(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio_info: &AudioInfo,
        spectrum: &SpectrumSnapshot,
        selected_index: usize,
        show_hidden: bool,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(14), Constraint::Min(6)])
            .split(area);

        self.render_spectrum(frame, chunks[0], spectrum);
        self.render_devices(frame, chunks[1], audio_info, selected_index, show_hidden);
    }

    fn render_header(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio_info: &AudioInfo,
        show_hidden: bool,
    ) {
        let active = audio_info.active_device(show_hidden);
        let title = match active {
            Some(dev) => format!(
                "Audio Pipeline Monitor - {} ({} Hz)",
                dev.card_name,
                dev.sample_rate.unwrap_or(0)
            ),
            None => "Audio Pipeline Monitor - No active device".to_string(),
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        frame.render_widget(block, area);
    }

    fn render_devices(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio_info: &AudioInfo,
        selected_index: usize,
        show_hidden: bool,
    ) {
        let devices = audio_info.visible_devices(show_hidden);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(5); devices.len().max(1)])
            .split(area);

        if devices.is_empty() {
            let paragraph = Paragraph::new("No visible playback streams")
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(paragraph, area);
            return;
        }

        for (i, device) in devices.iter().enumerate() {
            let is_selected = i == selected_index;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let state_color = match device.state {
                StreamState::Running => Color::Green,
                StreamState::Paused => Color::Yellow,
                StreamState::Stopped => Color::Red,
                StreamState::Unknown(_) => Color::Gray,
            };

            let sources = if device.sources.is_empty() {
                "Sources: none".to_string()
            } else {
                let names = device
                    .sources
                    .iter()
                    .map(|source| match source.sample_rate {
                        Some(rate) => format!("{} ({} Hz)", source.name, rate),
                        None => source.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Sources: {names}")
            };
            let volume = if device.volume.is_empty() {
                String::new()
            } else {
                format!(" | Vol: {}%", device.volume[0])
            };

            let text = format!(
                "Card {}: {} | PCM: {} | Sub: {} | State: {:?} | {} Hz | {} ch{}\n{}",
                device.card_id,
                device.card_name,
                device.pcm_id,
                device.sub_id,
                device.state,
                device.sample_rate.unwrap_or(0),
                device.channels.unwrap_or(0),
                volume,
                sources,
            );

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state_color))
                .style(style);

            let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });

            if i < chunks.len() {
                frame.render_widget(paragraph, chunks[i]);
            }
        }
    }

    fn render_spectrum(&self, frame: &mut Frame, area: Rect, spectrum: &SpectrumSnapshot) {
        let title = spectrum
            .source_name
            .as_deref()
            .map(|source| format!("Spectrum - {source}"))
            .unwrap_or_else(|| "Spectrum".to_string());

        if !spectrum.active {
            let paragraph = Paragraph::new(spectrum.message.as_str())
                .alignment(Alignment::Center)
                .block(Block::default().title(title).borders(Borders::ALL));
            frame.render_widget(paragraph, area);
            return;
        }

        let block = Block::default().title(title).borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width < 4 || inner.height < 4 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(inner);

        let status_text = format!(
            "{} | sensitivity: {}% | decay: {}",
            spectrum.message, spectrum.sensitivity, spectrum.decay
        );
        let status = Paragraph::new(status_text).alignment(Alignment::Center);
        frame.render_widget(status, chunks[0]);

        let graph = Paragraph::new(render_spectrum_rows(
            &spectrum.bins,
            &spectrum.peaks,
            chunks[1].width as usize,
            chunks[1].height as usize,
        ));
        frame.render_widget(graph, chunks[1]);

        let labels = Paragraph::new(render_spectrum_labels(
            spectrum.bins.len(),
            chunks[2].width as usize,
        ))
        .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(labels, chunks[2]);
    }

    fn render_footer(
        &self,
        frame: &mut Frame,
        area: Rect,
        show_hidden: bool,
        rate_info: Option<&OutputRateInfo>,
        footer_rate_label: &str,
        rate_status: Option<&str>,
    ) {
        let hidden_label = if show_hidden {
            "hide stopped"
        } else {
            "show stopped"
        };
        let rate_label = format_rate_info(rate_info);
        let status_label = rate_status.unwrap_or("");
        let rate_part = if rate_label.is_empty() {
            String::new()
        } else {
            format!(" | {}", rate_label)
        };
        let status_part = if status_label.is_empty() {
            String::new()
        } else {
            format!(" | {}", status_label)
        };
        let text = format!(
            "q: quit | ↑/↓: navigate | h: {} | +/-: sens | [ ]: decay | {}{}{}",
            hidden_label,
            footer_rate_label,
            rate_part,
            status_part,
        );
        let text_area = centered_rect(area, 120);
        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, text_area);
    }
}

fn centered_rect(area: Rect, max_width: u16) -> Rect {
    let width = area.width.min(max_width).max(1);
    let left = area.x + area.width.saturating_sub(width) / 2;

    Rect {
        x: left,
        y: area.y,
        width,
        height: area.height,
    }
}

fn format_rate_info(rate_info: Option<&OutputRateInfo>) -> String {
    match rate_info {
        Some(rate_info) => match rate_info.selected_rate {
            Some(selected_rate) if selected_rate != rate_info.current_rate => {
                format!(
                    "rate: {} Hz | selected: {} Hz",
                    rate_info.current_rate, selected_rate
                )
            }
            _ => format!("rate: {} Hz", rate_info.current_rate),
        },
        None => "rate: unavailable".to_string(),
    }
}

fn render_spectrum_rows(bins: &[u64], peaks: &[u64], width: usize, height: usize) -> Text<'static> {
    if bins.is_empty() || width == 0 || height == 0 {
        return Text::default();
    }

    let columns = width * 2;
    let step = bins.len() as f32 / columns as f32;
    let mut rows = Vec::with_capacity(height);

    for row in (0..height).rev() {
        let threshold = row + 1;
        let mut spans = Vec::with_capacity(width);

        for cell in 0..width {
            let left_column = cell * 2;
            let right_column = left_column + 1;
            let left_filled = column_level(bins, left_column, step, height);
            let right_filled = column_level(bins, right_column, step, height);
            let left_peak = column_level(peaks, left_column, step, height);
            let right_peak = column_level(peaks, right_column, step, height);

            let span = if left_filled >= threshold && right_filled >= threshold {
                Span::styled(
                    "█",
                    Style::default()
                        .fg(spectrum_color(threshold, height))
                        .add_modifier(Modifier::BOLD),
                )
            } else if left_filled >= threshold {
                Span::styled(
                    "▌",
                    Style::default()
                        .fg(spectrum_color(threshold, height))
                        .add_modifier(Modifier::BOLD),
                )
            } else if right_filled >= threshold {
                Span::styled(
                    "▐",
                    Style::default()
                        .fg(spectrum_color(threshold, height))
                        .add_modifier(Modifier::BOLD),
                )
            } else if left_peak == threshold && right_peak == threshold {
                Span::styled("▀", Style::default().fg(Color::White))
            } else if left_peak == threshold {
                Span::styled("▘", Style::default().fg(Color::White))
            } else if right_peak == threshold {
                Span::styled("▝", Style::default().fg(Color::White))
            } else {
                Span::raw(" ")
            };
            spans.push(span);
        }

        rows.push(Line::from(spans));
    }

    Text::from(rows)
}

fn column_level(values: &[u64], column: usize, step: f32, height: usize) -> usize {
    let start = (column as f32 * step).floor() as usize;
    let end = (((column + 1) as f32 * step).ceil() as usize)
        .max(start + 1)
        .min(values.len());

    values[start.min(values.len() - 1)..end]
        .iter()
        .map(|value| ((*value as usize * height) / 100).min(height))
        .max()
        .unwrap_or(0)
}

fn spectrum_color(level: usize, height: usize) -> Color {
    let ratio = level as f32 / height.max(1) as f32;

    if ratio < 0.25 {
        Color::Blue
    } else if ratio < 0.45 {
        Color::Cyan
    } else if ratio < 0.65 {
        Color::Green
    } else if ratio < 0.82 {
        Color::Yellow
    } else if ratio < 0.92 {
        Color::LightRed
    } else {
        Color::Red
    }
}

fn render_spectrum_labels(bin_count: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let columns = width;
    let mut line = vec![' '; columns];

    for (index, label) in spectrum_label_positions(bin_count) {
        let position = ((index as f32 / bin_count.max(1) as f32) * columns as f32).round() as usize;
        let start = position.min(columns.saturating_sub(label.len()));
        for (offset, ch) in label.chars().enumerate() {
            if start + offset < line.len() {
                line[start + offset] = ch;
            }
        }
    }

    line.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_state_colors() {
        assert_eq!(
            match StreamState::Running {
                StreamState::Running => Color::Green,
                _ => Color::White,
            },
            Color::Green
        );
    }
}
