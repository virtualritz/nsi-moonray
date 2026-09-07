//! `mnry`'s argument surface.
//!
//! Modelled on `rdl`, the `renderdl` replacement in
//! `virtualritz/delight-helpers`: the same subcommands, the same
//! frame-sequence syntax, the same short options where they mean the
//! same thing. Someone who renders ɴsɪ with 3Delight should not have to
//! learn a second command to render it with MoonRay.
//!
//! What is *not* here is `rdl`'s `--collective` and `--cloud`. Those
//! name 3Delight's distributed rendering; MoonRay has neither, and an
//! option that is accepted and ignored is worse than one that is not
//! there.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub fn parse() -> Cli {
    Cli::parse()
}

#[derive(Parser)]
#[command(
    name = "mnry",
    bin_name = "mnry",
    about = "Renders or converts ɴsɪ streams with MoonRay",
    max_term_width = 120,
    version
)]
pub struct Cli {
    #[arg(
        display_order = 10,
        long,
        short,
        action = clap::ArgAction::Count,
        help = "Verbosity level (-v verbose, -vv very verbose, etc.)",
    )]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Render(Render),
    Cat(Cat),
    Watch(Watch),

    #[command(
        name = "generate-completions",
        about = "Generate completion scripts for various shells",
        display_order = 9999
    )]
    GenerateCompletions {
        #[arg(
            help = "The shell to generate completions for",
            value_parser = clap::builder::PossibleValuesParser::new([
                "bash", "elvish", "fish", "powershell", "zsh",
            ])
        )]
        shell: String,
    },
}

// Where MoonRay's scene classes come from. Flattened into the
// subcommands that render, so a plain comment rather than a doc one:
// clap takes a flattened struct's doc comment as the *parent
// command's* description, which put this paragraph where `render`'s
// summary belongs.
#[derive(Parser, Clone)]
pub struct Renderer {
    #[arg(
        long,
        env = "NSI_MOONRAY_DSO",
        value_name = "DIR",
        help = "MoonRay's rdl2dso directory",
        long_help = "MoonRay's rdl2dso directory\n\
            Every scene class is loaded from here. Without it MoonRay \
            resolves nothing and renders an empty frame rather than \
            reporting, so this is worth getting right.",
        value_hint = clap::ValueHint::DirPath
    )]
    pub dso_path: Option<PathBuf>,

    #[arg(
        long,
        short,
        value_name = "THREADS",
        help = "Render using this many THREADS",
        long_help = "Render using this many THREADS\n\
            Without it MoonRay uses every core on the machine."
    )]
    pub threads: Option<usize>,
}

#[derive(Parser, Clone)]
#[command(
    arg_required_else_help = true,
    about = "Render ɴsɪ stream(s) or .rdla scene(s) with MoonRay"
)]
pub struct Render {
    #[arg(
        name = "FILE",
        index = 1,
        help = "The scene FILE(s) to render",
        long_help = "The scene FILE(s) to render\n\
            An ɴsɪ stream is parsed and flushed; an .rdla is used as it \
            stands. Which it is comes from the content, not the name.\n\
            Frame number placeholders are specified using @[padding]:\n\
            foo.@.nsi   ➞  foo.1.nsi, foo.2.nsi, …\n\
            foo.@4.nsi  ➞  foo.0001.nsi, foo.0002.nsi, …",
        value_hint = clap::ValueHint::FilePath
    )]
    pub file: Vec<String>,

    #[command(flatten)]
    pub renderer: Renderer,

    #[arg(
        long,
        short,
        value_name = "FRAMES",
        help = "FRAME(s) to render – 1,2,10-20,40-30@2",
        long_help = "FRAME(s) to render\n\
            They can be specified individually:\n\
            1,2,3,5,8,13\n\
            Or as a sequence:\n\
            10-15    ➞  10, 11, 12, 13, 14, 15\n\
            With an optional step size:\n\
            10-20@2  ➞  10, 12, 14, 16, 18, 20\n\
            Step size is always positive.\n\
            To render a sequence backwards specify the range in reverse:\n\
            42-33@3  ➞  42, 39, 36, 33\n\
            With binary splitting. Useful to quickly check if a sequence\n\
            has ‘issues’ in some frames:\n\
            10-20@b  ➞  10, 20, 15, 12, 17, 11, 13, 16, 18, 14, 19\n\
            The last frame of a sequence will be omitted if\n\
            the specified step size does not touch it:\n\
            80-70@4  ➞  80, 76, 72"
    )]
    pub frames: Option<String>,

    #[arg(
        long,
        short,
        value_name = "IMAGE",
        help = "Write the image to IMAGE",
        long_help = "Write the image to IMAGE instead of where the \
            scene's own output driver points",
        value_hint = clap::ValueHint::FilePath
    )]
    pub output: Option<PathBuf>,

    #[arg(long, short, help = "Print rendering progress")]
    pub progress: bool,

    #[arg(
        long,
        short,
        action = clap::ArgAction::Count,
        help = "Report what the frame cost: (-s, -ss)",
        long_help = "Report what the frame cost\n\
            -s   ➞  render time\n\
            -ss  ➞  render time, plus what MoonRay had to rebuild",
    )]
    pub statistics: u8,

    #[arg(
        long,
        help = "Do not render, just print what would be done",
        long_help = "Do not render, just print the name of the file(s) \
            that would be rendered"
    )]
    pub dry_run: bool,
}

#[derive(Parser)]
#[command(
    arg_required_else_help = true,
    about = "Dump the input as an .rdla scene to stdout or a file"
)]
pub struct Cat {
    #[arg(
        name = "FILE",
        index = 1,
        help = "The ɴsɪ FILE to dump",
        value_hint = clap::ValueHint::FilePath
    )]
    pub file: PathBuf,

    #[arg(
        long,
        short,
        value_name = "OUTPUT",
        help = "Write to OUTPUT",
        long_help = "Write to OUTPUT instead of stdout",
        value_hint = clap::ValueHint::FilePath
    )]
    pub output: Option<PathBuf>,

    #[arg(
        long,
        help = "Dump what an interactive render would build",
        long_help = "Dump what an interactive render would build\n\
            A batch flush leaves out geometry the scene hid, since \
            nothing will show it again. An interactive one keeps it, \
            switched off, so unhiding stays an attribute edit."
    )]
    pub interactive: bool,

    #[arg(
        long,
        short,
        help = "Print what could not be translated",
        long_help = "Print what could not be translated\n\
            MoonRay runs no OSL and has no analytic quadrics, so a \
            scene generally loses something. This says what."
    )]
    pub limitations: bool,
}

#[derive(Parser)]
#[command(
    arg_required_else_help = true,
    about = "Watch folder(s) for new scenes and render them with MoonRay"
)]
pub struct Watch {
    #[arg(
        name = "FOLDER",
        index = 1,
        help = "The FOLDER(s) to watch for scene file(s) to render",
        value_hint = clap::ValueHint::DirPath
    )]
    pub folder: Vec<PathBuf>,

    #[command(flatten)]
    pub renderer: Renderer,

    #[arg(
        long,
        short,
        help = "Recurse into the given folder(s)",
        long_help = "Recurse into the given folder(s) when looking for \
            new files to render"
    )]
    pub recursive: bool,
}
