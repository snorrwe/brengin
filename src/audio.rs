use parking_lot::Mutex;
use std::{path::Path, sync::Arc};

use kira::{
    backend::{cpal::CpalBackend, DefaultBackend},
    sound::static_sound::StaticSoundData,
};

use crate::{
    assets::{Assets, AssetsPlugin, Handle},
    Plugin,
};

pub use kira;

pub struct AudioPlugin;

type AM<T = DefaultBackend> = kira::AudioManager<T>;

pub struct AudioManager {
    manager: Arc<Mutex<AM>>,
}

impl AudioManager {
    fn new(manager: AM) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
        }
    }

    pub fn play(&self, audio: &Audio) -> Sound {
        let mut manager = self.manager.lock();
        let sound = manager
            .play(audio.data.clone())
            .expect("Failed to play audio");
        Sound { handle: sound }
    }
}

pub struct Sound {
    handle: kira::sound::static_sound::StaticSoundHandle,
}

impl Sound {
    pub fn handle(&self) -> &kira::sound::static_sound::StaticSoundHandle {
        &self.handle
    }

    pub fn handle_mut(&mut self) -> &mut kira::sound::static_sound::StaticSoundHandle {
        &mut self.handle
    }
}

pub struct Audio {
    data: StaticSoundData,
}

impl Audio {
    pub fn load_audio_bytes(
        bytes: &'static [u8],
        assets: &mut Assets<Self>,
    ) -> anyhow::Result<Handle<Self>> {
        let data = StaticSoundData::from_cursor(std::io::Cursor::new(bytes))?;
        let res = Self { data };
        let handle = assets.insert(res);
        Ok(handle)
    }

    pub fn load_audio_file(
        p: impl AsRef<Path>,
        assets: &mut Assets<Self>,
    ) -> anyhow::Result<Handle<Self>> {
        let data = StaticSoundData::from_file(p)?;
        let res = Self { data };
        let handle = assets.insert(res);
        Ok(handle)
    }
}

impl Plugin for AudioPlugin {
    fn build(self, app: &mut crate::App) {
        let manager = AM::<CpalBackend>::new(kira::AudioManagerSettings::default())
            .expect("Failed to initialize audio");

        app.insert_resource(AudioManager::new(manager));
        app.add_plugin(AssetsPlugin::<Audio>::default());
    }
}
