use crate::domain::Result;
use rusqlite::params;

use super::ClaudeEnvStore;

impl ClaudeEnvStore {
    pub fn seed_default_theme(&self) -> Result<usize> {
        let connection = self.connection()?;

        connection.execute(
            "
            INSERT INTO themes (
                name, is_active,
                active_pane_border, inactive_pane_border,
                project_highlight, session_highlight,
                status_badge_bg, status_badge_fg,
                hint_key_fg, hint_text_fg,
                meta_text_fg, modal_border
            ) VALUES (
                'dracula', 1,
                'Cyan', 'DarkGray',
                'Yellow', 'Magenta',
                'LightMagenta', 'Black',
                'White', 'Gray',
                'DarkGray', 'Cyan'
            )
            ON CONFLICT(name) DO NOTHING
            ",
            [],
        )?;

        let active_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM themes WHERE is_active = 1",
            [],
            |row| row.get(0),
        )?;

        if active_count == 0 {
            connection.execute("UPDATE themes SET is_active = 1 WHERE name = 'dracula'", [])?;
        }

        Ok(1)
    }

    pub fn active_theme(&self) -> Result<Option<super::Theme>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "
            SELECT
                name, is_active,
                active_pane_border, inactive_pane_border,
                project_highlight, session_highlight,
                status_badge_bg, status_badge_fg,
                hint_key_fg, hint_text_fg,
                meta_text_fg, modal_border
            FROM themes
            WHERE is_active = 1
            LIMIT 1
            ",
        )?;

        let mut rows = statement.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(super::Theme {
                name: row.get(0)?,
                is_active: row.get::<_, bool>(1)?,
                active_pane_border: row.get(2)?,
                inactive_pane_border: row.get(3)?,
                project_highlight: row.get(4)?,
                session_highlight: row.get(5)?,
                status_badge_bg: row.get(6)?,
                status_badge_fg: row.get(7)?,
                hint_key_fg: row.get(8)?,
                hint_text_fg: row.get(9)?,
                meta_text_fg: row.get(10)?,
                modal_border: row.get(11)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_themes(&self) -> Result<Vec<super::Theme>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "
            SELECT
                name, is_active,
                active_pane_border, inactive_pane_border,
                project_highlight, session_highlight,
                status_badge_bg, status_badge_fg,
                hint_key_fg, hint_text_fg,
                meta_text_fg, modal_border
            FROM themes
            ORDER BY name
            ",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(super::Theme {
                name: row.get(0)?,
                is_active: row.get::<_, bool>(1)?,
                active_pane_border: row.get(2)?,
                inactive_pane_border: row.get(3)?,
                project_highlight: row.get(4)?,
                session_highlight: row.get(5)?,
                status_badge_bg: row.get(6)?,
                status_badge_fg: row.get(7)?,
                hint_key_fg: row.get(8)?,
                hint_text_fg: row.get(9)?,
                meta_text_fg: row.get(10)?,
                modal_border: row.get(11)?,
            })
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn upsert_theme(&self, theme: &super::Theme) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "
            INSERT INTO themes (
                name, is_active,
                active_pane_border, inactive_pane_border,
                project_highlight, session_highlight,
                status_badge_bg, status_badge_fg,
                hint_key_fg, hint_text_fg,
                meta_text_fg, modal_border
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(name) DO UPDATE SET
                is_active = excluded.is_active,
                active_pane_border = excluded.active_pane_border,
                inactive_pane_border = excluded.inactive_pane_border,
                project_highlight = excluded.project_highlight,
                session_highlight = excluded.session_highlight,
                status_badge_bg = excluded.status_badge_bg,
                status_badge_fg = excluded.status_badge_fg,
                hint_key_fg = excluded.hint_key_fg,
                hint_text_fg = excluded.hint_text_fg,
                meta_text_fg = excluded.meta_text_fg,
                modal_border = excluded.modal_border
            ",
            params![
                theme.name,
                theme.is_active,
                theme.active_pane_border,
                theme.inactive_pane_border,
                theme.project_highlight,
                theme.session_highlight,
                theme.status_badge_bg,
                theme.status_badge_fg,
                theme.hint_key_fg,
                theme.hint_text_fg,
                theme.meta_text_fg,
                theme.modal_border,
            ],
        )?;
        Ok(())
    }

    pub fn set_active_theme(&self, name: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute("UPDATE themes SET is_active = 0", [])?;
        let rows = connection.execute("UPDATE themes SET is_active = 1 WHERE name = ?1", [name])?;
        if rows == 0 {
            return Err(crate::domain::StetsonError::ThemeNotFound(name.to_string()));
        }
        Ok(())
    }
}
