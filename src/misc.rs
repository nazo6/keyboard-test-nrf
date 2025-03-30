use rktk_drivers_common::mouse::paw3395;

pub const PAW3395_CONFIG: paw3395::config::Config = paw3395::config::Config {
    mode: paw3395::config::HP_MODE,
    lift_cutoff: paw3395::config::LiftCutoff::_2mm,
};

pub fn translate_key_position(row: usize, col: usize) -> Option<(usize, usize)> {
    #[cfg(feature = "left")]
    {
        Some((row, 7 - col))
    }

    #[cfg(feature = "right")]
    {
        Some((row, col))
    }
}
