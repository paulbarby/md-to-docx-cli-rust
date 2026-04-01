use docx_rs::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const ORDERED_ABSTRACT_NUMBERING_ID: usize = 1;
const BULLET_ABSTRACT_NUMBERING_ID: usize = 2;
const FIRST_DYNAMIC_NUMBERING_ID: usize = 2;

#[derive(Debug, Clone)]
struct CliOptions {
    input: PathBuf,
    output: Option<PathBuf>,
    overwrite: bool,
    title: Option<String>,
    author: Option<String>,
}

#[derive(Debug, Clone)]
struct ConversionJob {
    input: PathBuf,
    output: PathBuf,
}

#[derive(Debug)]
enum ParseOutcome {
    Help,
    Run(CliOptions),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TextStyle {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    size: Option<usize>,
}

#[derive(Debug, Clone)]
enum InlineComponent {
    Text(String, TextStyle),
    Break,
    SoftBreak,
    Link {
        label: String,
        url: String,
        style: TextStyle,
    },
    Image {
        alt: String,
        url: String,
    },
}

#[derive(Debug, Clone, Copy)]
enum BlockType {
    Paragraph,
    Heading(u8),
    CodeBlock,
}

#[derive(Debug, Clone)]
struct BlockBuffer {
    kind: BlockType,
    components: Vec<InlineComponent>,
    list_depth: usize,
    quote_depth: usize,
    list_numbering_id: Option<usize>,
    is_list_item_paragraph: bool,
}

#[derive(Debug, Clone)]
struct ListState {
    numbering_id: usize,
}

#[derive(Debug, Clone)]
struct ItemState {
    marker_applied: bool,
}

#[derive(Debug, Clone)]
struct ImageState {
    url: String,
    alt: String,
}

struct Renderer {
    docx: Docx,
    current_block: Option<BlockBuffer>,
    current_link: Option<String>,
    current_image: Option<ImageState>,
    style: TextStyle,
    list_stack: Vec<ListState>,
    item_stack: Vec<ItemState>,
    quote_depth: usize,
    mono_fonts: RunFonts,
    next_numbering_id: usize,
}

fn main() {
    match parse_args_from(env::args_os()) {
        Ok(ParseOutcome::Help) => {
            print_help();
        }
        Ok(ParseOutcome::Run(options)) => {
            if let Err(error) = execute(options) {
                eprintln!("error: {error}");
                process::exit(1);
            }
        }
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!();
            print_help();
            process::exit(2);
        }
    }
}

fn execute(options: CliOptions) -> Result<()> {
    if options.input.is_file() {
        let output = options
            .output
            .clone()
            .unwrap_or_else(|| default_output_path(&options.input));

        write_document(&options.input, &output, &options)?;
        println!("Wrote {}", output.display());
        return Ok(());
    }

    if options.input.is_dir() {
        let jobs = build_directory_jobs(&options)?;

        if jobs.is_empty() {
            return Err(format!(
                "no markdown files found under {}",
                options.input.display()
            )
            .into());
        }

        for job in &jobs {
            write_document(&job.input, &job.output, &options)?;
        }

        if let Some(output_root) = &options.output {
            println!("Wrote {} files under {}", jobs.len(), output_root.display());
        } else {
            println!(
                "Wrote {} files next to their source markdown files under {}",
                jobs.len(),
                options.input.display()
            );
        }

        return Ok(());
    }

    Err(format!("input path does not exist: {}", options.input.display()).into())
}

fn write_document(input: &Path, output: &Path, options: &CliOptions) -> Result<()> {
    if output.exists() && !options.overwrite {
        return Err(format!(
            "output already exists: {} (pass --overwrite to replace it)",
            output.display()
        )
        .into());
    }

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let markdown = fs::read_to_string(input)?;
    let document = build_document(&markdown, &options);
    let file = File::create(output)?;
    document.build().pack(file)?;

    Ok(())
}

fn build_directory_jobs(options: &CliOptions) -> Result<Vec<ConversionJob>> {
    let inputs = collect_markdown_files(&options.input)?;

    if let Some(output_root) = &options.output {
        if output_root.exists() && output_root.is_file() {
            return Err(format!(
                "directory mode requires an output directory, but a file exists at {}",
                output_root.display()
            )
            .into());
        }
    }

    let mut seen_outputs = HashSet::new();
    let mut jobs = Vec::with_capacity(inputs.len());

    for input in inputs {
        let output = directory_output_path(&options.input, &input, options.output.as_deref())?;

        if !seen_outputs.insert(output.clone()) {
            return Err(format!("multiple inputs would write to {}", output.display()).into());
        }

        if output.exists() && !options.overwrite {
            return Err(format!(
                "output already exists: {} (pass --overwrite to replace it)",
                output.display()
            )
            .into());
        }

        jobs.push(ConversionJob { input, output });
    }

    Ok(jobs)
}

fn collect_markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(directory) = directories.pop() {
        let mut entries = fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file() && is_markdown_file(&path) {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| {
            value.eq_ignore_ascii_case("md")
                || value.eq_ignore_ascii_case("markdown")
                || value.eq_ignore_ascii_case("mdown")
        })
        .unwrap_or(false)
}

fn directory_output_path(
    input_root: &Path,
    input_file: &Path,
    output_root: Option<&Path>,
) -> Result<PathBuf> {
    let relative = input_file.strip_prefix(input_root).map_err(|_| {
        format!(
            "failed to derive a relative path for {} under {}",
            input_file.display(),
            input_root.display()
        )
    })?;

    let mut output = if let Some(output_root) = output_root {
        output_root.join(relative)
    } else {
        input_file.to_path_buf()
    };

    output.set_extension("docx");
    Ok(output)
}

fn build_document(markdown: &str, options: &CliOptions) -> Docx {
    let body_fonts = RunFonts::new()
        .ascii("Aptos")
        .hi_ansi("Aptos")
        .east_asia("Aptos")
        .cs("Aptos");

    let mono_fonts = RunFonts::new()
        .ascii("Consolas")
        .hi_ansi("Consolas")
        .east_asia("Consolas")
        .cs("Consolas");

    let mut document = Docx::new()
        .default_fonts(body_fonts)
        .default_size(22)
        .add_abstract_numbering(build_bullet_abstract_numbering());

    if let Some(title) = &options.title {
        document = document
            .add_paragraph(
                Paragraph::new()
                    .align(AlignmentType::Center)
                    .add_run(Run::new().add_text(title).bold().size(32)),
            )
            .custom_property("title", title.clone());

        if let Some(author) = &options.author {
            document = document
                .add_paragraph(
                    Paragraph::new()
                        .align(AlignmentType::Center)
                        .add_run(Run::new().add_text(author).italic().size(22)),
                )
                .custom_property("author", author.clone());
        }

        document = document.add_paragraph(Paragraph::new());
    } else if let Some(author) = &options.author {
        document = document.custom_property("author", author.clone());
    }

    Renderer::new(document, mono_fonts).render(markdown)
}

fn parse_args_from<I>(args: I) -> std::result::Result<ParseOutcome, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut overwrite = false;
    let mut title: Option<String> = None;
    let mut author: Option<String> = None;

    let mut pending = args.into_iter().skip(1).peekable();
    while let Some(argument) = pending.next() {
        let value = argument.to_string_lossy();
        match value.as_ref() {
            "-h" | "--help" => return Ok(ParseOutcome::Help),
            "--overwrite" => overwrite = true,
            "-o" | "--output" => {
                let next = pending
                    .next()
                    .ok_or_else(|| "missing value after --output".to_string())?;
                output = Some(PathBuf::from(next));
            }
            "--title" => {
                let next = pending
                    .next()
                    .ok_or_else(|| "missing value after --title".to_string())?;
                title = Some(next.to_string_lossy().into_owned());
            }
            "--author" => {
                let next = pending
                    .next()
                    .ok_or_else(|| "missing value after --author".to_string())?;
                author = Some(next.to_string_lossy().into_owned());
            }
            "--" => {
                for remainder in pending {
                    push_positional(&mut input, &mut output, PathBuf::from(remainder))?;
                }
                break;
            }
            _ if value.starts_with('-') => {
                return Err(format!("unknown flag: {value}"));
            }
            _ => {
                push_positional(&mut input, &mut output, PathBuf::from(argument))?;
            }
        }
    }

    let input = input.ok_or_else(|| "missing input markdown file or directory".to_string())?;

    Ok(ParseOutcome::Run(CliOptions {
        input,
        output,
        overwrite,
        title,
        author,
    }))
}

fn push_positional(
    input: &mut Option<PathBuf>,
    output: &mut Option<PathBuf>,
    value: PathBuf,
) -> std::result::Result<(), String> {
    if input.is_none() {
        *input = Some(value);
        return Ok(());
    }

    if output.is_none() {
        *output = Some(value);
        return Ok(());
    }

    Err("too many positional arguments".to_string())
}

fn default_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("output");

    input.with_file_name(format!("{stem}.docx"))
}

fn print_help() {
    println!(
        "md2docx\n\n\
Convert Markdown documents into formatted Word .docx files.\n\n\
USAGE:\n\
    md2docx <input.md> [output.docx] [--title TEXT] [--author TEXT] [--overwrite]\n\
    md2docx <input.md> --output <output.docx> [--title TEXT] [--author TEXT] [--overwrite]\n\n\
    md2docx <input-dir> [output-dir] [--title TEXT] [--author TEXT] [--overwrite]\n\
    md2docx <input-dir> --output <output-dir> [--title TEXT] [--author TEXT] [--overwrite]\n\n\
OPTIONS:\n\
    -o, --output <path>   Explicit output path\n\
        --title <text>    Insert a title page heading at the top of the document\n\
        --author <text>   Insert the author beneath the title and save as a custom property\n\
        --overwrite       Replace an existing output file\n\
    -h, --help            Show this help\n\n\
DIRECTORY MODE:\n\
    If the input path is a directory, md2docx converts all .md, .markdown, and .mdown\n\
    files under that directory recursively. Without --output, each .docx file is written\n\
    next to its source Markdown file. With --output, the input directory structure is\n\
    mirrored under the output directory.\n"
    );
}

impl Renderer {
    fn new(docx: Docx, mono_fonts: RunFonts) -> Self {
        Self {
            docx,
            current_block: None,
            current_link: None,
            current_image: None,
            style: TextStyle::default(),
            list_stack: Vec::new(),
            item_stack: Vec::new(),
            quote_depth: 0,
            mono_fonts,
            next_numbering_id: FIRST_DYNAMIC_NUMBERING_ID,
        }
    }

    fn render(mut self, markdown: &str) -> Docx {
        let parser = Parser::new_ext(markdown, Options::all());
        for event in parser {
            self.handle_event(event);
        }

        self.flush_current_block();
        self.docx
    }

    fn handle_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.handle_start(tag),
            Event::End(tag) => self.handle_end(tag),
            Event::Text(text) => self.push_text(&text, self.style),
            Event::Code(text) => {
                let mut style = self.style;
                style.code = true;
                self.push_text(&text, style);
            }
            Event::SoftBreak => self.push_soft_break(),
            Event::HardBreak => self.push_break(),
            Event::Rule => {
                self.flush_current_block();
                let document = std::mem::take(&mut self.docx);
                self.docx = document.add_paragraph(
                    Paragraph::new().add_run(Run::new().add_text(
                        "----------------------------------------",
                    )),
                );
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                let mut style = self.style;
                style.code = true;
                self.push_text(&html, style);
            }
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                let mut style = self.style;
                style.code = true;
                self.push_text(&math, style);
            }
            Event::FootnoteReference(label) => {
                self.push_text(&format!("[{label}]"), self.style);
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                self.push_text(marker, self.style);
            }
        }
    }

    fn handle_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.start_block(BlockType::Paragraph),
            Tag::Heading { level, .. } => self.start_block(BlockType::Heading(heading_to_u8(level))),
            Tag::BlockQuote(_) => {
                self.flush_current_block();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush_current_block();
                self.start_block(BlockType::CodeBlock);

                if let CodeBlockKind::Fenced(language) = kind {
                    let info = language.trim();
                    if !info.is_empty() {
                        let mut style = TextStyle::default();
                        style.code = true;
                        self.push_text(&format!("[{info}]\n"), style);
                    }
                }
            }
            Tag::List(start) => {
                self.flush_current_block();
                let numbering_id = self.allocate_list_numbering(start.is_some(), start);
                self.list_stack.push(ListState {
                    numbering_id,
                });
            }
            Tag::Item => {
                self.flush_current_block();
                self.item_stack.push(ItemState {
                    marker_applied: false,
                });
            }
            Tag::Emphasis => self.style.italic = true,
            Tag::Strong => self.style.bold = true,
            Tag::Strikethrough => self.style.strike = true,
            Tag::Link { dest_url, .. } => {
                self.current_link = Some(dest_url.to_string());
            }
            Tag::Image { dest_url, .. } => {
                self.current_image = Some(ImageState {
                    url: dest_url.to_string(),
                    alt: String::new(),
                });
            }
            Tag::Table(_) => self.start_block(BlockType::Paragraph),
            Tag::TableHead => {}
            Tag::TableRow => {}
            Tag::TableCell => {}
            Tag::FootnoteDefinition(label) => {
                self.start_block(BlockType::Paragraph);
                self.push_text(&format!("[{label}] "), self.style);
            }
            Tag::DefinitionList => self.flush_current_block(),
            Tag::DefinitionListTitle => self.start_block(BlockType::Paragraph),
            Tag::DefinitionListDefinition => self.start_block(BlockType::Paragraph),
            Tag::MetadataBlock(_) => self.start_block(BlockType::CodeBlock),
            Tag::HtmlBlock => self.start_block(BlockType::CodeBlock),
            Tag::Superscript | Tag::Subscript => {}
        }
    }

    fn handle_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::HtmlBlock => {
                self.flush_current_block();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_current_block();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.flush_current_block();
                self.list_stack.pop();
            }
            TagEnd::Item => {
                self.flush_current_block();
                self.item_stack.pop();
            }
            TagEnd::Emphasis => self.style.italic = false,
            TagEnd::Strong => self.style.bold = false,
            TagEnd::Strikethrough => self.style.strike = false,
            TagEnd::Link => {
                self.current_link = None;
            }
            TagEnd::Image => {
                if let Some(image) = self.current_image.take() {
                    self.ensure_text_block(BlockType::Paragraph);
                    if let Some(block) = self.current_block.as_mut() {
                        block.components.push(InlineComponent::Image {
                            alt: image.alt,
                            url: image.url,
                        });
                    }
                }
            }
            TagEnd::TableCell => self.push_text(" | ", self.style),
            TagEnd::TableRow => self.push_break(),
            TagEnd::Table => self.flush_current_block(),
            TagEnd::TableHead => self.push_break(),
            TagEnd::FootnoteDefinition => self.flush_current_block(),
            TagEnd::DefinitionList => self.flush_current_block(),
            TagEnd::DefinitionListTitle => self.push_text(": ", self.style),
            TagEnd::DefinitionListDefinition => self.flush_current_block(),
            TagEnd::MetadataBlock(_) => self.flush_current_block(),
            TagEnd::Superscript | TagEnd::Subscript => {}
        }
    }

    fn start_block(&mut self, kind: BlockType) {
        self.flush_current_block();
        self.current_block = Some(self.create_block_buffer(kind));
    }

    fn ensure_text_block(&mut self, default_kind: BlockType) {
        if self.current_block.is_none() {
            self.current_block = Some(self.create_block_buffer(default_kind));
        }
    }

    fn create_block_buffer(&mut self, kind: BlockType) -> BlockBuffer {
        let mut list_numbering_id = None;
        let mut is_list_item_paragraph = false;

        if let Some(item) = self.item_stack.last_mut()
            && !item.marker_applied
        {
            item.marker_applied = true;
            is_list_item_paragraph = true;
            list_numbering_id = self.list_stack.last().map(|list| list.numbering_id);
        }

        BlockBuffer {
            kind,
            components: Vec::new(),
            list_depth: self.item_stack.len(),
            quote_depth: self.quote_depth,
            list_numbering_id,
            is_list_item_paragraph,
        }
    }

    fn allocate_list_numbering(&mut self, ordered: bool, start: Option<u64>) -> usize {
        let numbering_id = self.next_numbering_id;
        self.next_numbering_id += 1;

        let abstract_numbering_id = if ordered {
            ORDERED_ABSTRACT_NUMBERING_ID
        } else {
            BULLET_ABSTRACT_NUMBERING_ID
        };

        let mut numbering = Numbering::new(numbering_id, abstract_numbering_id);
        if ordered {
            let start_value = start.unwrap_or(1);
            if start_value > 1 {
                numbering = numbering.add_override(LevelOverride::new(0).start(start_value as usize));
            }
        }

        let document = std::mem::take(&mut self.docx);
        self.docx = document.add_numbering(numbering);
        numbering_id
    }

    fn push_text(&mut self, text: &str, style: TextStyle) {
        if text.is_empty() {
            return;
        }

        if let Some(image) = self.current_image.as_mut() {
            image.alt.push_str(text);
            return;
        }

        self.ensure_text_block(BlockType::Paragraph);

        if let Some(url) = &self.current_link {
            if let Some(block) = self.current_block.as_mut() {
                block.components.push(InlineComponent::Link {
                    label: text.to_string(),
                    url: url.clone(),
                    style,
                });
            }
            return;
        }

        if let Some(block) = self.current_block.as_mut() {
            block
                .components
                .push(InlineComponent::Text(text.to_string(), style));
        }
    }

    fn push_soft_break(&mut self) {
        if matches!(self.current_block.as_ref().map(|block| block.kind), Some(BlockType::CodeBlock)) {
            self.push_break();
        } else if self.current_image.is_some() || self.current_link.is_some() {
            self.push_text(" ", self.style);
        } else {
            self.ensure_text_block(BlockType::Paragraph);
            if let Some(block) = self.current_block.as_mut() {
                block.components.push(InlineComponent::SoftBreak);
            }
        }
    }

    fn push_break(&mut self) {
        if let Some(image) = self.current_image.as_mut() {
            image.alt.push(' ');
            return;
        }

        if self.current_link.is_some() {
            self.push_text(" ", self.style);
            return;
        }

        self.ensure_text_block(BlockType::Paragraph);
        if let Some(block) = self.current_block.as_mut() {
            block.components.push(InlineComponent::Break);
        }
    }

    fn flush_current_block(&mut self) {
        let Some(block) = self.current_block.take() else {
            return;
        };

        if block.components.is_empty() || block_is_blank(&block) {
            return;
        }

        let segments = split_components_on_soft_breaks(block.components);
        let segment_count = segments.len();

        for (index, components) in segments.into_iter().enumerate() {
            if components.is_empty() || components_are_blank(&components) {
                continue;
            }

            let is_continuation = index > 0;
            let is_last_segment = index + 1 == segment_count;
            let mut paragraph = Paragraph::new();

            match block.kind {
                BlockType::Paragraph => {
                    paragraph = paragraph.line_spacing(body_spacing(
                        block.is_list_item_paragraph,
                        is_continuation,
                        is_last_segment,
                    ));
                }
                BlockType::Heading(level) => {
                    paragraph = paragraph
                        .style(heading_style(level))
                        .outline_lvl(level.saturating_sub(1) as usize)
                        .keep_next(true)
                        .keep_lines(true)
                        .line_spacing(heading_spacing(level));
                }
                BlockType::CodeBlock => {
                    paragraph = paragraph
                        .color("333333")
                        .line_spacing(code_block_spacing());
                }
            }

            let quote_indent = (block.quote_depth as i32) * 360;
            if block.is_list_item_paragraph && !is_continuation {
                if let Some(numbering_id) = block.list_numbering_id {
                    paragraph = paragraph.numbering(
                        NumberingId::new(numbering_id),
                        IndentLevel::new(block.list_depth.saturating_sub(1)),
                    );
                }
            } else if block.list_depth > 0 {
                let left = (block.list_depth as i32) * 420 + quote_indent;
                paragraph = paragraph.indent(Some(left), None, None, None);
            } else if block.quote_depth > 0 {
                paragraph = paragraph.indent(Some(quote_indent), None, None, None).color("666666");
            }

            if matches!(block.kind, BlockType::CodeBlock) {
                let left = 280 + (block.list_depth as i32) * 420 + quote_indent;
                paragraph = paragraph.indent(Some(left), None, None, None);
            }

            for component in components {
                paragraph = self.append_component(paragraph, component, block.kind);
            }

            let document = std::mem::take(&mut self.docx);
            self.docx = document.add_paragraph(paragraph);
        }
    }

    fn append_component(
        &self,
        paragraph: Paragraph,
        component: InlineComponent,
        block_kind: BlockType,
    ) -> Paragraph {
        match component {
            InlineComponent::Text(text, style) => {
                let style = block_text_style(style, block_kind);
                append_text_runs(paragraph, &text, self.build_run(style))
            }
            InlineComponent::Break => paragraph.add_run(Run::new().add_break(BreakType::TextWrapping)),
            InlineComponent::SoftBreak => paragraph,
            InlineComponent::Link { label, url, style } => {
                let style = block_text_style(style, block_kind);

                let mut paragraph = paragraph;
                let mut first = true;
                for piece in split_text_preserving_breaks(&label) {
                    match piece {
                        TextPiece::Text(segment) => {
                            if segment.is_empty() {
                                continue;
                            }

                            let mut run = self.build_run(style).add_text(segment);
                            run = run.color("0563C1").underline("single");
                            paragraph = paragraph.add_hyperlink(
                                Hyperlink::new(url.clone(), HyperlinkType::External).add_run(run),
                            );
                            first = false;
                        }
                        TextPiece::Break => {
                            if !first {
                                paragraph = paragraph
                                    .add_run(Run::new().add_break(BreakType::TextWrapping));
                            }
                        }
                    }
                }
                paragraph
            }
            InlineComponent::Image { alt, url } => {
                let label = if alt.trim().is_empty() {
                    "[image]".to_string()
                } else {
                    format!("[image: {}]", alt.trim())
                };

                paragraph.add_hyperlink(
                    Hyperlink::new(url, HyperlinkType::External)
                        .add_run(Run::new().add_text(label).color("0563C1").underline("single")),
                )
            }
        }
    }

    fn build_run(&self, style: TextStyle) -> Run {
        let mut run = Run::new();

        if style.bold {
            run = run.bold();
        }
        if style.italic {
            run = run.italic();
        }
        if style.strike {
            run = run.strike();
        }
        if style.code {
            run = run.fonts(self.mono_fonts.clone());
        }
        if let Some(size) = style.size {
            run = run.size(size);
        } else if style.code {
            run = run.size(20);
        }

        run
    }
}

#[derive(Debug, Clone)]
enum TextPiece<'a> {
    Text(&'a str),
    Break,
}

fn append_text_runs(mut paragraph: Paragraph, text: &str, run_template: Run) -> Paragraph {
    for piece in split_text_preserving_breaks(text) {
        match piece {
            TextPiece::Text(segment) => {
                if segment.is_empty() {
                    continue;
                }
                paragraph = paragraph.add_run(run_template.clone().add_text(segment));
            }
            TextPiece::Break => {
                paragraph = paragraph.add_run(Run::new().add_break(BreakType::TextWrapping));
            }
        }
    }
    paragraph
}

fn split_text_preserving_breaks(text: &str) -> Vec<TextPiece<'_>> {
    let mut pieces = Vec::new();
    let mut start = 0usize;

    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            pieces.push(TextPiece::Text(&text[start..index]));
            pieces.push(TextPiece::Break);
            start = index + ch.len_utf8();
        }
    }

    pieces.push(TextPiece::Text(&text[start..]));
    pieces
}

fn heading_style(level: u8) -> &'static str {
    match level {
        1 => "Heading1",
        2 => "Heading2",
        3 => "Heading3",
        4 => "Heading4",
        5 => "Heading5",
        _ => "Heading6",
    }
}

fn block_text_style(mut style: TextStyle, block_kind: BlockType) -> TextStyle {
    match block_kind {
        BlockType::Paragraph => style,
        BlockType::Heading(level) => {
            style.bold = true;
            style.size = Some(heading_font_size(level));
            style
        }
        BlockType::CodeBlock => {
            style.code = true;
            style
        }
    }
}

fn heading_font_size(level: u8) -> usize {
    match level {
        1 => 36,
        2 => 32,
        3 => 28,
        4 => 24,
        5 => 22,
        _ => 20,
    }
}

fn heading_spacing(level: u8) -> LineSpacing {
    match level {
        1 => LineSpacing::new().before(320).after(140),
        2 => LineSpacing::new().before(260).after(120),
        3 => LineSpacing::new().before(220).after(100),
        4 => LineSpacing::new().before(180).after(80),
        5 => LineSpacing::new().before(160).after(60),
        _ => LineSpacing::new().before(140).after(40),
    }
}

fn body_spacing(is_list_item: bool, is_continuation: bool, is_last_segment: bool) -> LineSpacing {
    let after = if is_last_segment {
        if is_list_item { 100 } else { 160 }
    } else if is_continuation {
        110
    } else {
        120
    };

    LineSpacing::new()
        .line_rule(LineSpacingType::Auto)
        .line(300)
        .after(after)
}

fn split_components_on_soft_breaks(components: Vec<InlineComponent>) -> Vec<Vec<InlineComponent>> {
    let mut segments = vec![Vec::new()];

    for component in components {
        if matches!(component, InlineComponent::SoftBreak) {
            segments.push(Vec::new());
        } else if let Some(segment) = segments.last_mut() {
            segment.push(component);
        }
    }

    segments
}

fn components_are_blank(components: &[InlineComponent]) -> bool {
    components.iter().all(|component| match component {
        InlineComponent::Break | InlineComponent::SoftBreak => true,
        InlineComponent::Text(text, _) => text.trim().is_empty(),
        InlineComponent::Link { label, .. } => label.trim().is_empty(),
        InlineComponent::Image { .. } => false,
    })
}

fn code_block_spacing() -> LineSpacing {
    LineSpacing::new()
        .line_rule(LineSpacingType::Auto)
        .line(280)
        .before(80)
        .after(120)
}

fn build_bullet_abstract_numbering() -> AbstractNumbering {
    let bullet_markers = ["•", "◦", "▪"];
    let mut numbering = AbstractNumbering::new(BULLET_ABSTRACT_NUMBERING_ID);

    for level in 0..9usize {
        let left = 420 * (level as i32 + 1);
        numbering = numbering.add_level(
            Level::new(
                level,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new(bullet_markers[level % bullet_markers.len()]),
                LevelJc::new("left"),
            )
            .indent(Some(left), Some(SpecialIndentType::Hanging(420)), None, None),
        );
    }

    numbering
}

fn heading_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn block_is_blank(block: &BlockBuffer) -> bool {
    components_are_blank(&block.components)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_default_output_path() {
        let input = PathBuf::from("docs/spec.md");
        assert_eq!(default_output_path(&input), PathBuf::from("docs/spec.docx"));
    }

    #[test]
    fn parses_directory_arguments_without_default_output() {
        let result = parse_args_from([OsString::from("md2docx"), OsString::from("technical_specs")])
            .expect("arguments should parse");

        let ParseOutcome::Run(options) = result else {
            panic!("expected run outcome");
        };

        assert_eq!(options.input, PathBuf::from("technical_specs"));
        assert_eq!(options.output, None);
    }

    #[test]
    fn parses_basic_arguments() {
        let result = parse_args_from([
            OsString::from("md2docx"),
            OsString::from("guide.md"),
            OsString::from("--title"),
            OsString::from("Guide"),
            OsString::from("--overwrite"),
        ])
        .expect("arguments should parse");

        let ParseOutcome::Run(options) = result else {
            panic!("expected run outcome");
        };

        assert_eq!(options.input, PathBuf::from("guide.md"));
        assert_eq!(options.output, None);
        assert!(options.overwrite);
        assert_eq!(options.title.as_deref(), Some("Guide"));
    }

    #[test]
    fn derives_directory_output_path() {
        let output = directory_output_path(
            Path::new("technical_specs"),
            Path::new("technical_specs/ai_agents_components/README.md"),
            Some(Path::new("out")),
        )
        .expect("output path should resolve");

        assert_eq!(
            output,
            PathBuf::from("out/ai_agents_components/README.docx")
        );
    }

    #[test]
    fn heading_blocks_apply_explicit_font_sizes() {
        let style = block_text_style(TextStyle::default(), BlockType::Heading(2));

        assert!(style.bold);
        assert_eq!(style.size, Some(32));
    }

    #[test]
    fn code_blocks_force_monospace_styling() {
        let mut style = TextStyle::default();
        style.bold = true;

        let styled = block_text_style(style, BlockType::CodeBlock);

        assert!(styled.bold);
        assert!(styled.code);
        assert_eq!(styled.size, None);
    }

    #[test]
    fn body_spacing_adds_paragraph_gap() {
        let xml = body_spacing(false, false, true).build();
        let xml = String::from_utf8(xml).expect("spacing xml should be utf-8");

        assert!(xml.contains("w:after=\"160\""));
        assert!(xml.contains("w:line=\"300\""));
    }

    #[test]
    fn soft_break_segments_get_continuation_gap() {
        let xml = body_spacing(false, false, false).build();
        let xml = String::from_utf8(xml).expect("spacing xml should be utf-8");

        assert!(xml.contains("w:after=\"120\""));
    }

    #[test]
    fn splits_components_on_soft_breaks() {
        let segments = split_components_on_soft_breaks(vec![
            InlineComponent::Text("alpha".to_string(), TextStyle::default()),
            InlineComponent::SoftBreak,
            InlineComponent::Text("beta".to_string(), TextStyle::default()),
        ]);

        assert_eq!(segments.len(), 2);
        assert!(matches!(segments[0][0], InlineComponent::Text(_, _)));
        assert!(matches!(segments[1][0], InlineComponent::Text(_, _)));
    }

    #[test]
    fn bullet_numbering_uses_word_bullet_format() {
        let xml = build_bullet_abstract_numbering().build();
        let xml = String::from_utf8(xml).expect("numbering xml should be utf-8");

        assert!(xml.contains("w:numFmt w:val=\"bullet\""));
        assert!(xml.contains("w:lvlText w:val=\"•\""));
    }
}
