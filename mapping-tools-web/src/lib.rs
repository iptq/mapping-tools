#![recursion_limit = "256"]

#[macro_use]
extern crate yew;

use std::io::Cursor;

use libosu::prelude::*;
use mapping_tools::{copy_hitsounds, ExtraOpts};
use wasm_bindgen::prelude::*;
use yew::{
    events::ChangeData,
    services::reader::{FileData, ReaderService, ReaderTask},
    Component, ComponentLink, Html, ShouldRender,
};

pub struct Model {
    link: ComponentLink<Model>,
    reader: ReaderService,
    tasks: Vec<ReaderTask>,
    src: String,
    dst: String,
    output: String,
}

#[derive(Copy, Clone)]
pub enum WhichField {
    Src,
    Dst,
}

pub enum Msg {
    Copy,
    Change(WhichField, ChangeData),
    Loaded(WhichField, FileData),
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        Model {
            link,
            reader: ReaderService::new(),
            tasks: Vec::new(),
            src: String::new(),
            dst: String::new(),
            output: String::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Copy => {
                let src = Beatmap::parse(Cursor::new(&self.src)).unwrap();
                let dst = Beatmap::parse(Cursor::new(&self.dst)).unwrap();
                let mut dsts = vec![dst];
                copy_hitsounds(&src, &mut dsts, ExtraOpts { leniency: 2 }).unwrap();
                self.output = dsts[0].to_string();
                true
            }
            Msg::Change(field, ChangeData::Files(files)) if files.length() >= 1 => {
                println!("change!");
                let file = files.get(0).expect("just checked");
                let task = self
                    .reader
                    .read_file(
                        file,
                        self.link.callback(move |data| Msg::Loaded(field, data)),
                    )
                    .unwrap();
                self.tasks.push(task);
                true
            }
            Msg::Loaded(field, data) => {
                let content = String::from_utf8(data.content).unwrap();
                match field {
                    WhichField::Src => self.src = content,
                    WhichField::Dst => self.dst = content,
                }
                true
            }
            _ => true,
        }
    }

    fn change(&mut self, _: Self::Properties) -> ShouldRender {
        false
    }

    fn view(&self) -> Html {
        html! {
            <div>
                <h1>{"Hitsound Copier"}</h1>
                <p>{"From:"} <input type="file" onchange=self.link.callback(|e| Msg::Change(WhichField::Src, e)) /></p>
                <p>{"To:"} <input type="file" onchange=self.link.callback(|e| Msg::Change(WhichField::Dst, e)) /></p>
                <p><input type="submit" value="Copy!" onclick=self.link.callback(|_| Msg::Copy) /></p>

                <pre>{&self.output}</pre>
            </div>
        }
    }
}

#[wasm_bindgen(start)]
pub fn run_app() {
    println!("SHIET");
    yew::start_app::<Model>();
}
