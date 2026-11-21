//! fig's semantic actions: keys and the pointer produce these, and one place carries them out.

/// What a key or a gesture asks of a figure.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The cursor a number of samples or bands along.
    Step(i32),
    /// The cursor to the next or previous series.
    Series(i32),
    /// The cursor to the next or previous peak.
    Peak(i32),
    /// The cursor to a neighbouring node.
    Neighbour(i32),
    /// The cursor to the first sample.
    First,
    /// The cursor to the last sample.
    Last,
    /// The cursor to the view's left edge (-1), middle (0) or right edge (1).
    ViewEdge(i8),
    /// Zoom by a factor, about a point in logical px or the cursor.
    Zoom {
        /// Above 1 zooms in.
        factor: f64,
        /// About where.
        at: Option<(f32, f32)>,
    },
    /// The whole data again.
    ZoomReset,
    /// Pan by logical px.
    Scroll {
        /// Rightward.
        dx: f32,
        /// Downward.
        dy: f32,
    },
    /// The view centred on the cursor.
    Centre,
    /// Open the detail panel.
    Detail,
    /// Focus the next or previous stop.
    Focus(i32),
    /// Search the labels.
    Find,
    /// The next or previous search hit.
    FindNext(i32),
    /// Hide the cursor's series.
    Hide,
    /// Show the cursor's series alone.
    Only,
    /// Show every series.
    ShowAll,
    /// Undo a view change.
    Undo,
    /// Redo one.
    Redo,
    /// Show the data as a table.
    Table,
    /// The settings.
    Settings,
    /// Export a PNG or an SVG.
    Export,
    /// Copy the figure as a picture.
    CopyImage,
    /// Read the data again.
    Refresh,
    /// A dashboard panel full size.
    FullPanel,
    /// The dashboard's time range.
    TimeRange,
    /// Set or clear the reference.
    Reference,
    /// Cancel, close, clear.
    Escape,
    /// The keys.
    Help,
}
