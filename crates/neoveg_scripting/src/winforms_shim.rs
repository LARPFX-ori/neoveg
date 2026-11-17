//! Camada `System.Windows.Forms` própria, compilada junto com o script quando
//! o WinForms real não existe (Linux, macOS e Windows sem o runtime
//! WindowsDesktop).
//!
//! Sala limpa: nomes de tipos, membros e valores de enumeração vêm da
//! documentação pública da Microsoft (learn.microsoft.com, referência da API
//! `System.Windows.Forms`); a implementação é própria e não lê o código-fonte
//! do WinForms. Cobre o que scripts de edição usam: `MessageBox`, `Form` com
//! `ShowDialog`, `Label`, `TextBox`, `Button`, `CheckBox`, `RadioButton`,
//! `ComboBox`, `ListBox`, `NumericUpDown`, `GroupBox`, `Panel`,
//! `OpenFileDialog`, `SaveFileDialog`, `FolderBrowserDialog` e `Application`.
//! As janelas são desenhadas pelo Neoveg (ver `script_ui.rs`); `Form.Show()`
//! sem modal e desenho próprio (`OnPaint`, `Graphics`) não existem.

pub(crate) const WINFORMS_SHIM_CS: &str = r#"using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Drawing;
using System.IO;
using System.Text.Json;
using ScriptPortal.Vegas;

namespace System.Windows.Forms
{
    public enum DialogResult { None = 0, OK = 1, Cancel = 2, Abort = 3, Retry = 4, Ignore = 5, Yes = 6, No = 7, TryAgain = 10, Continue = 11 }
    public enum MessageBoxButtons { OK = 0, OKCancel = 1, AbortRetryIgnore = 2, YesNoCancel = 3, YesNo = 4, RetryCancel = 5, CancelTryContinue = 6 }
    public enum MessageBoxIcon { None = 0, Hand = 16, Stop = 16, Error = 16, Question = 32, Exclamation = 48, Warning = 48, Asterisk = 64, Information = 64 }
    public enum MessageBoxDefaultButton { Button1 = 0, Button2 = 256, Button3 = 512, Button4 = 768 }
    [Flags]
    public enum MessageBoxOptions { DefaultDesktopOnly = 0x20000, RightAlign = 0x80000, RtlReading = 0x100000, ServiceNotification = 0x200000 }
    public enum FormBorderStyle { None = 0, FixedSingle = 1, Fixed3D = 2, FixedDialog = 3, Sizable = 4, FixedToolWindow = 5, SizableToolWindow = 6 }
    public enum FormStartPosition { Manual = 0, CenterScreen = 1, WindowsDefaultLocation = 2, WindowsDefaultBounds = 3, CenterParent = 4 }
    public enum ComboBoxStyle { Simple = 0, DropDown = 1, DropDownList = 2 }
    public enum DockStyle { None = 0, Top = 1, Bottom = 2, Left = 3, Right = 4, Fill = 5 }
    [Flags]
    public enum AnchorStyles { None = 0, Top = 1, Bottom = 2, Left = 4, Right = 8 }
    public enum ScrollBars { None = 0, Horizontal = 1, Vertical = 2, Both = 3 }
    public enum HorizontalAlignment { Left = 0, Right = 1, Center = 2 }
    public enum SelectionMode { None = 0, One = 1, MultiSimple = 2, MultiExtended = 3 }
    public enum CloseReason { None = 0, WindowsShutDown = 1, MdiFormClosing = 2, UserClosing = 3, TaskManagerClosing = 4, FormOwnerClosing = 5, ApplicationExitCall = 6 }

    public interface IWin32Window { IntPtr Handle { get; } }

    public interface IButtonControl
    {
        DialogResult DialogResult { get; set; }
        void NotifyDefault(bool value);
        void PerformClick();
    }

    public class FormClosingEventArgs : CancelEventArgs
    {
        public FormClosingEventArgs(CloseReason closeReason, bool cancel) : base(cancel) { CloseReason = closeReason; }
        public CloseReason CloseReason { get; }
    }

    public class FormClosedEventArgs : EventArgs
    {
        public FormClosedEventArgs(CloseReason closeReason) { CloseReason = closeReason; }
        public CloseReason CloseReason { get; }
    }

    public delegate void FormClosingEventHandler(object sender, FormClosingEventArgs e);
    public delegate void FormClosedEventHandler(object sender, FormClosedEventArgs e);

    public static class Application
    {
        public static void EnableVisualStyles() { }
        public static void SetCompatibleTextRenderingDefault(bool defaultValue) { }
        public static void DoEvents() { }
        public static void Run(Form mainForm) { mainForm?.ShowDialog(); }
    }

    internal static class Json
    {
        internal static string Str(JsonElement e, string name) =>
            e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.String ? p.GetString() : null;

        internal static bool Bool(JsonElement e, string name) =>
            e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.True;

        internal static int Int(JsonElement e, string name, int fallback) =>
            e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.Number && p.TryGetInt32(out int v) ? v : fallback;

        internal static double Num(JsonElement e, string name, double fallback) =>
            e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.Number && p.TryGetDouble(out double v) ? v : fallback;

        internal static string Kind(JsonDocument d) => Str(d.RootElement, "kind");
    }

    public static class MessageBox
    {
        public static DialogResult Show(string text) => ShowCore(text, "", MessageBoxButtons.OK, MessageBoxIcon.None);
        public static DialogResult Show(string text, string caption) => ShowCore(text, caption, MessageBoxButtons.OK, MessageBoxIcon.None);
        public static DialogResult Show(string text, string caption, MessageBoxButtons buttons) => ShowCore(text, caption, buttons, MessageBoxIcon.None);
        public static DialogResult Show(string text, string caption, MessageBoxButtons buttons, MessageBoxIcon icon) => ShowCore(text, caption, buttons, icon);
        public static DialogResult Show(string text, string caption, MessageBoxButtons buttons, MessageBoxIcon icon, MessageBoxDefaultButton defaultButton) => ShowCore(text, caption, buttons, icon);
        public static DialogResult Show(string text, string caption, MessageBoxButtons buttons, MessageBoxIcon icon, MessageBoxDefaultButton defaultButton, MessageBoxOptions options) => ShowCore(text, caption, buttons, icon);
        public static DialogResult Show(IWin32Window owner, string text) => ShowCore(text, "", MessageBoxButtons.OK, MessageBoxIcon.None);
        public static DialogResult Show(IWin32Window owner, string text, string caption) => ShowCore(text, caption, MessageBoxButtons.OK, MessageBoxIcon.None);
        public static DialogResult Show(IWin32Window owner, string text, string caption, MessageBoxButtons buttons) => ShowCore(text, caption, buttons, MessageBoxIcon.None);
        public static DialogResult Show(IWin32Window owner, string text, string caption, MessageBoxButtons buttons, MessageBoxIcon icon) => ShowCore(text, caption, buttons, icon);
        public static DialogResult Show(IWin32Window owner, string text, string caption, MessageBoxButtons buttons, MessageBoxIcon icon, MessageBoxDefaultButton defaultButton) => ShowCore(text, caption, buttons, icon);

        private static string IconName(MessageBoxIcon icon)
        {
            switch ((int)icon)
            {
                case 16: return "Error";
                case 32: return "Question";
                case 48: return "Warning";
                case 64: return "Information";
                default: return "None";
            }
        }

        private static DialogResult FirstResult(MessageBoxButtons buttons)
        {
            switch (buttons)
            {
                case MessageBoxButtons.AbortRetryIgnore: return DialogResult.Abort;
                case MessageBoxButtons.YesNoCancel:
                case MessageBoxButtons.YesNo: return DialogResult.Yes;
                case MessageBoxButtons.RetryCancel: return DialogResult.Retry;
                case MessageBoxButtons.CancelTryContinue: return DialogResult.Cancel;
                default: return DialogResult.OK;
            }
        }

        private static DialogResult ShowCore(string text, string caption, MessageBoxButtons buttons, MessageBoxIcon icon)
        {
            var doc = NeovegHostCalls.Ui(new Dictionary<string, object>
            {
                ["kind"] = "message_box",
                ["text"] = text ?? "",
                ["caption"] = caption ?? "",
                ["buttons"] = buttons.ToString(),
                ["icon"] = IconName(icon),
            });
            if (doc != null)
            {
                using (doc)
                {
                    if (Json.Kind(doc) == "message_box"
                        && Enum.TryParse(Json.Str(doc.RootElement, "result"), out DialogResult r))
                    {
                        return r;
                    }
                }
            }
            // sem interface (CLI): registra o texto e responde com o primeiro botão
            NeovegHostCalls.Current?.ShowMessage(string.IsNullOrEmpty(caption) ? (text ?? "") : caption + ": " + text);
            return FirstResult(buttons);
        }
    }

    public class Control : IWin32Window, IDisposable
    {
        private string _text = "";

        public Control() { Controls = new ControlCollection(this); }

        public string Name { get; set; } = "";
        public virtual string Text
        {
            get => _text;
            set
            {
                string v = value ?? "";
                if (_text != v)
                {
                    _text = v;
                    OnTextChanged(EventArgs.Empty);
                }
            }
        }
        public int Left { get; set; }
        public int Top { get; set; }
        public int Width { get; set; } = 100;
        public int Height { get; set; } = 23;
        public int Right => Left + Width;
        public int Bottom => Top + Height;
        public Point Location { get => new Point(Left, Top); set { Left = value.X; Top = value.Y; } }
        public Size Size { get => new Size(Width, Height); set { Width = value.Width; Height = value.Height; } }
        public virtual Size ClientSize { get => Size; set => Size = value; }
        public Rectangle Bounds { get => new Rectangle(Left, Top, Width, Height); set { Location = value.Location; Size = value.Size; } }
        public bool Visible { get; set; } = true;
        public bool Enabled { get; set; } = true;
        public object Tag { get; set; }
        public int TabIndex { get; set; }
        public bool TabStop { get; set; } = true;
        public bool AutoSize { get; set; }
        public AnchorStyles Anchor { get; set; } = AnchorStyles.Top | AnchorStyles.Left;
        public DockStyle Dock { get; set; }
        public Color ForeColor { get; set; }
        public Color BackColor { get; set; }
        public Control Parent { get; internal set; }
        public ControlCollection Controls { get; }
        public IntPtr Handle => IntPtr.Zero;
        public bool IsDisposed => false;

        public event EventHandler Click;
        public event EventHandler TextChanged;

        protected virtual void OnClick(EventArgs e) => Click?.Invoke(this, e);
        protected virtual void OnTextChanged(EventArgs e) => TextChanged?.Invoke(this, e);

        public void SuspendLayout() { }
        public void ResumeLayout() { }
        public void ResumeLayout(bool performLayout) { }
        public void PerformLayout() { }
        public bool Focus() => true;
        public void Select() { }
        public void BringToFront() { }
        public void SendToBack() { }
        public void Show() { Visible = true; }
        public void Hide() { Visible = false; }
        public void Refresh() { }
        public void Invalidate() { }
        public void Update() { }
        public void Dispose() { }

        // --- descrição para o Neoveg (não faz parte da API do WinForms) ---
        internal virtual string Kind => null;
        internal bool HasClick => Click != null;
        internal bool HasTextChanged => TextChanged != null;
        internal virtual bool Notify => HasClick;
        internal void RaiseClick() => OnClick(EventArgs.Empty);
        internal virtual void Describe(Dictionary<string, object> d) { }
        internal virtual void Apply(JsonElement v) { }

        public class ControlCollection : IEnumerable<Control>
        {
            private readonly Control _owner;
            private readonly List<Control> _items = new List<Control>();

            internal ControlCollection(Control owner) { _owner = owner; }

            public int Count => _items.Count;
            public Control this[int index] => _items[index];
            public Control this[string key] => _items.Find(c => c.Name == key);

            public void Add(Control value)
            {
                if (value == null || value == _owner || _items.Contains(value))
                {
                    return;
                }
                value.Parent?.Controls.Remove(value);
                value.Parent = _owner;
                _items.Add(value);
            }

            public void AddRange(Control[] controls)
            {
                if (controls == null)
                {
                    return;
                }
                foreach (var c in controls)
                {
                    Add(c);
                }
            }

            public void Remove(Control value)
            {
                if (value != null && _items.Remove(value))
                {
                    value.Parent = null;
                }
            }

            public void RemoveAt(int index) => Remove(_items[index]);
            public void Clear()
            {
                foreach (var c in _items)
                {
                    c.Parent = null;
                }
                _items.Clear();
            }
            public bool Contains(Control control) => _items.Contains(control);
            public int IndexOf(Control control) => _items.IndexOf(control);
            public IEnumerator<Control> GetEnumerator() => _items.GetEnumerator();
            IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        }
    }

    public class Label : Control
    {
        public Label() { Height = 23; }
        internal override string Kind => "label";
    }

    public class GroupBox : Control
    {
        public GroupBox() { Width = 200; Height = 100; }
        internal override string Kind => "group_box";
    }

    public class Panel : Control
    {
        public Panel() { Width = 200; Height = 100; }
        public bool AutoScroll { get; set; }
        internal override string Kind => "panel";
    }

    public class TextBox : Control
    {
        public bool Multiline { get; set; }
        public bool ReadOnly { get; set; }
        public char PasswordChar { get; set; }
        public bool UseSystemPasswordChar { get; set; }
        public int MaxLength { get; set; } = 32767;
        public ScrollBars ScrollBars { get; set; }
        public bool WordWrap { get; set; } = true;
        public HorizontalAlignment TextAlign { get; set; }
        public string[] Lines
        {
            get => Text.Replace("\r\n", "\n").Split('\n');
            set => Text = value == null ? "" : string.Join(Environment.NewLine, value);
        }
        public void AppendText(string text) { Text += text; }
        public void Clear() { Text = ""; }
        public void SelectAll() { }
        internal override string Kind => "text_box";
        internal override bool Notify => HasClick || HasTextChanged;
        internal override void Describe(Dictionary<string, object> d)
        {
            d["multiline"] = Multiline;
            d["read_only"] = ReadOnly;
            d["password"] = PasswordChar != '\0' || UseSystemPasswordChar;
        }
        internal override void Apply(JsonElement v)
        {
            string t = Json.Str(v, "text");
            if (t != null && !ReadOnly)
            {
                Text = MaxLength > 0 && t.Length > MaxLength ? t.Substring(0, MaxLength) : t;
            }
        }
    }

    public class Button : Control, IButtonControl
    {
        public Button() { Width = 75; Height = 23; }
        public DialogResult DialogResult { get; set; }
        public void NotifyDefault(bool value) { }
        public void PerformClick() => OnClick(EventArgs.Empty);
        internal override string Kind => "button";
        internal override bool Notify => true;
        internal override void Describe(Dictionary<string, object> d)
        {
            d["dialog_result"] = DialogResult == DialogResult.None ? null : DialogResult.ToString();
        }
    }

    public class CheckBox : Control
    {
        private bool _checked;
        public bool Checked
        {
            get => _checked;
            set
            {
                if (_checked != value)
                {
                    _checked = value;
                    CheckedChanged?.Invoke(this, EventArgs.Empty);
                }
            }
        }
        public event EventHandler CheckedChanged;
        internal override string Kind => "check_box";
        internal override bool Notify => HasClick || CheckedChanged != null;
        internal override void Describe(Dictionary<string, object> d) { d["checked"] = Checked; }
        internal override void Apply(JsonElement v) { Checked = Json.Bool(v, "checked"); }
    }

    public class RadioButton : Control
    {
        private bool _checked;
        public bool Checked
        {
            get => _checked;
            set
            {
                if (_checked != value)
                {
                    _checked = value;
                    CheckedChanged?.Invoke(this, EventArgs.Empty);
                }
            }
        }
        public event EventHandler CheckedChanged;
        internal override string Kind => "radio_button";
        internal override bool Notify => HasClick || CheckedChanged != null;
        internal override void Describe(Dictionary<string, object> d) { d["checked"] = Checked; }
        internal override void Apply(JsonElement v) { Checked = Json.Bool(v, "checked"); }
    }

    public abstract class ListControl : Control
    {
        private int _selected = -1;

        protected ListControl() { Items = new ObjectCollection(this); }

        public ObjectCollection Items { get; }
        public event EventHandler SelectedIndexChanged;

        public int SelectedIndex
        {
            get => _selected;
            set
            {
                int v = value < -1 || value >= Items.Count ? -1 : value;
                if (_selected != v)
                {
                    _selected = v;
                    SelectedIndexChanged?.Invoke(this, EventArgs.Empty);
                }
            }
        }

        public object SelectedItem
        {
            get => _selected >= 0 && _selected < Items.Count ? Items[_selected] : null;
            set => SelectedIndex = Items.IndexOf(value);
        }

        internal void ItemsChanged()
        {
            if (_selected >= Items.Count)
            {
                SelectedIndex = -1;
            }
        }

        internal override bool Notify => HasClick || SelectedIndexChanged != null || HasTextChanged;

        internal override void Describe(Dictionary<string, object> d)
        {
            var items = new List<string>();
            foreach (var it in Items)
            {
                items.Add(it?.ToString() ?? "");
            }
            d["items"] = items;
            d["selected"] = SelectedIndex;
        }

        internal override void Apply(JsonElement v) { SelectedIndex = Json.Int(v, "selected", -1); }

        public class ObjectCollection : IEnumerable<object>
        {
            private readonly ListControl _owner;
            private readonly List<object> _items = new List<object>();

            internal ObjectCollection(ListControl owner) { _owner = owner; }

            public int Count => _items.Count;
            public object this[int index] { get => _items[index]; set => _items[index] = value; }
            public int Add(object item)
            {
                _items.Add(item);
                return _items.Count - 1;
            }
            public void AddRange(object[] items)
            {
                if (items != null)
                {
                    _items.AddRange(items);
                }
            }
            public void Insert(int index, object item) => _items.Insert(index, item);
            public void Remove(object value)
            {
                _items.Remove(value);
                _owner.ItemsChanged();
            }
            public void RemoveAt(int index)
            {
                _items.RemoveAt(index);
                _owner.ItemsChanged();
            }
            public void Clear()
            {
                _items.Clear();
                _owner.ItemsChanged();
            }
            public bool Contains(object value) => _items.Contains(value);
            public int IndexOf(object value) => _items.IndexOf(value);
            public IEnumerator<object> GetEnumerator() => _items.GetEnumerator();
            IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        }
    }

    public class ComboBox : ListControl
    {
        public ComboBoxStyle DropDownStyle { get; set; } = ComboBoxStyle.DropDown;
        public int MaxDropDownItems { get; set; } = 8;
        public override string Text
        {
            get => SelectedItem?.ToString() ?? base.Text;
            set
            {
                int i = -1;
                for (int k = 0; k < Items.Count; k++)
                {
                    if ((Items[k]?.ToString() ?? "") == (value ?? ""))
                    {
                        i = k;
                        break;
                    }
                }
                if (i >= 0)
                {
                    SelectedIndex = i;
                }
                base.Text = value;
            }
        }
        internal override string Kind => "combo_box";
        internal override void Describe(Dictionary<string, object> d)
        {
            base.Describe(d);
            d["drop_down_list"] = DropDownStyle == ComboBoxStyle.DropDownList;
        }
    }

    public class ListBox : ListControl
    {
        public SelectionMode SelectionMode { get; set; } = SelectionMode.One;
        internal override string Kind => "list_box";
    }

    public class NumericUpDown : Control
    {
        private decimal _value;
        public NumericUpDown() { Width = 120; Height = 23; }
        public decimal Minimum { get; set; } = 0m;
        public decimal Maximum { get; set; } = 100m;
        public decimal Increment { get; set; } = 1m;
        public int DecimalPlaces { get; set; }
        public bool ThousandsSeparator { get; set; }
        public bool Hexadecimal { get; set; }
        public decimal Value
        {
            get => _value;
            set
            {
                if (value < Minimum || value > Maximum)
                {
                    throw new ArgumentOutOfRangeException(nameof(Value), "o valor está fora de Minimum..Maximum");
                }
                if (_value != value)
                {
                    _value = value;
                    ValueChanged?.Invoke(this, EventArgs.Empty);
                }
            }
        }
        public event EventHandler ValueChanged;
        public override string Text { get => _value.ToString(); set { } }
        internal override string Kind => "numeric";
        internal override bool Notify => HasClick || ValueChanged != null;
        internal override void Describe(Dictionary<string, object> d)
        {
            d["value"] = (double)_value;
            d["minimum"] = (double)Minimum;
            d["maximum"] = (double)Maximum;
            d["increment"] = (double)Increment;
            d["decimals"] = Math.Clamp(DecimalPlaces, 0, 10);
        }
        internal override void Apply(JsonElement v)
        {
            double n = Json.Num(v, "value", (double)_value);
            if (double.IsFinite(n))
            {
                decimal dv;
                try
                {
                    dv = Math.Round((decimal)n, Math.Clamp(DecimalPlaces, 0, 10));
                }
                catch (OverflowException)
                {
                    return;
                }
                Value = Math.Clamp(dv, Minimum, Maximum);
            }
        }
    }

    public class Form : Control
    {
        private Size? _clientSize;

        public Form() { Width = 300; Height = 300; }

        public DialogResult DialogResult { get; set; }
        public IButtonControl AcceptButton { get; set; }
        public IButtonControl CancelButton { get; set; }
        public FormBorderStyle FormBorderStyle { get; set; } = FormBorderStyle.Sizable;
        public FormStartPosition StartPosition { get; set; } = FormStartPosition.WindowsDefaultLocation;
        public bool MaximizeBox { get; set; } = true;
        public bool MinimizeBox { get; set; } = true;
        public bool ControlBox { get; set; } = true;
        public bool ShowIcon { get; set; } = true;
        public bool ShowInTaskbar { get; set; } = true;
        public bool TopMost { get; set; }
        public bool KeyPreview { get; set; }
        public double Opacity { get; set; } = 1.0;
        public Form Owner { get; set; }
        public bool Modal { get; private set; }

        // Width/Height incluem a borda; a área útil é ClientSize
        public override Size ClientSize
        {
            get => _clientSize ?? new Size(Math.Max(Width - 16, 0), Math.Max(Height - 39, 0));
            set => _clientSize = value;
        }

        public event EventHandler Load;
        public event EventHandler Shown;
        public event FormClosingEventHandler FormClosing;
        public event FormClosedEventHandler FormClosed;

        public void Activate() { }
        public void CenterToScreen() { }
        public new void Show() => throw new NotSupportedException("Form.Show() (janela não modal) não existe no Neoveg fora do Windows; use ShowDialog()");
        public void Close()
        {
            if (DialogResult == DialogResult.None)
            {
                DialogResult = DialogResult.Cancel;
            }
        }

        public DialogResult ShowDialog() => ShowDialog(null);

        public DialogResult ShowDialog(IWin32Window owner)
        {
            DialogResult = DialogResult.None;
            Modal = true;
            try
            {
                Load?.Invoke(this, EventArgs.Empty);
                Shown?.Invoke(this, EventArgs.Empty);
                for (int round = 0; round < 100000; round++)
                {
                    if (DialogResult == DialogResult.None)
                    {
                        var flat = new List<Control>();
                        var controls = new List<Dictionary<string, object>>();
                        Flatten(this, 0, 0, null, true, true, flat, controls, 0, new HashSet<Control>());
                        var doc = NeovegHostCalls.Ui(new Dictionary<string, object>
                        {
                            ["kind"] = "form",
                            ["title"] = Text ?? "",
                            ["width"] = ClientSize.Width,
                            ["height"] = ClientSize.Height,
                            ["controls"] = controls,
                            ["accept"] = IdOf(flat, AcceptButton),
                            ["cancel"] = IdOf(flat, CancelButton),
                        });
                        if (doc == null)
                        {
                            DialogResult = DialogResult.Cancel;
                            break;
                        }
                        using (doc)
                        {
                            var root = doc.RootElement;
                            if (Json.Kind(doc) != "form")
                            {
                                // sem interface: o formulário é cancelado
                                DialogResult = DialogResult.Cancel;
                                break;
                            }
                            if (root.TryGetProperty("values", out var values) && values.ValueKind == JsonValueKind.Array)
                            {
                                foreach (var v in values.EnumerateArray())
                                {
                                    int id = Json.Int(v, "id", -1);
                                    if (id >= 0 && id < flat.Count)
                                    {
                                        flat[id].Apply(v);
                                    }
                                }
                            }
                            int clicked = Json.Int(root, "clicked", -1);
                            if (clicked >= 0 && clicked < flat.Count)
                            {
                                var c = flat[clicked];
                                c.RaiseClick();
                                if (c is Button b && b.DialogResult != DialogResult.None && DialogResult == DialogResult.None)
                                {
                                    DialogResult = b.DialogResult;
                                }
                            }
                            if (Json.Bool(root, "closed") && DialogResult == DialogResult.None)
                            {
                                DialogResult = Enum.TryParse(Json.Str(root, "result"), out DialogResult r) && r != DialogResult.None
                                    ? r
                                    : DialogResult.Cancel;
                            }
                        }
                    }
                    if (DialogResult != DialogResult.None)
                    {
                        var closing = new FormClosingEventArgs(CloseReason.UserClosing, false);
                        FormClosing?.Invoke(this, closing);
                        if (closing.Cancel)
                        {
                            DialogResult = DialogResult.None;
                            continue;
                        }
                        FormClosed?.Invoke(this, new FormClosedEventArgs(CloseReason.UserClosing));
                        break;
                    }
                }
                if (DialogResult == DialogResult.None)
                {
                    DialogResult = DialogResult.Cancel;
                }
                return DialogResult;
            }
            finally
            {
                Modal = false;
            }
        }

        private static int? IdOf(List<Control> flat, IButtonControl b)
        {
            int i = b is Control c ? flat.IndexOf(c) : -1;
            return i >= 0 ? i : (int?)null;
        }

        // contêineres somam o deslocamento; ciclos e profundidade limitados (§0.5)
        private static void Flatten(Control parent, int ox, int oy, int? parentId, bool visible, bool enabled,
            List<Control> flat, List<Dictionary<string, object>> outList, int depth, HashSet<Control> seen)
        {
            if (depth > 32)
            {
                return;
            }
            foreach (var c in parent.Controls)
            {
                if (c == null || flat.Count >= 256 || !seen.Add(c))
                {
                    continue;
                }
                int id = flat.Count;
                flat.Add(c);
                bool vis = visible && c.Visible;
                bool en = enabled && c.Enabled;
                var d = new Dictionary<string, object>
                {
                    ["id"] = id,
                    ["kind"] = c.Kind,
                    ["parent"] = parentId,
                    ["text"] = c.Text ?? "",
                    ["x"] = ox + c.Left,
                    ["y"] = oy + c.Top,
                    ["w"] = c.Width,
                    ["h"] = c.Height,
                    ["enabled"] = en,
                    ["visible"] = vis,
                    ["notify"] = c.Notify,
                };
                c.Describe(d);
                outList.Add(d);
                if (c.Controls.Count > 0)
                {
                    Flatten(c, ox + c.Left, oy + c.Top, id, vis, en, flat, outList, depth + 1, seen);
                }
            }
        }
    }

    public abstract class CommonDialog : IDisposable
    {
        public object Tag { get; set; }
        public DialogResult ShowDialog() => ShowDialog(null);
        public DialogResult ShowDialog(IWin32Window owner) => RunDialog() ? DialogResult.OK : DialogResult.Cancel;
        protected abstract bool RunDialog();
        public virtual void Reset() { }
        public void Dispose() { }

        internal static string[] Ask(string mode, string title, string filter, string initialDir, string fileName, bool multiselect)
        {
            var doc = NeovegHostCalls.Ui(new Dictionary<string, object>
            {
                ["kind"] = "file_dialog",
                ["mode"] = mode,
                ["title"] = title ?? "",
                ["filter"] = filter ?? "",
                ["initial_dir"] = initialDir ?? "",
                ["file_name"] = fileName ?? "",
                ["multiselect"] = multiselect,
            });
            if (doc == null)
            {
                return null;
            }
            using (doc)
            {
                var root = doc.RootElement;
                if (Json.Kind(doc) != "file" || !Json.Bool(root, "ok")
                    || !root.TryGetProperty("paths", out var paths) || paths.ValueKind != JsonValueKind.Array)
                {
                    return null;
                }
                var list = new List<string>();
                foreach (var p in paths.EnumerateArray())
                {
                    if (p.ValueKind == JsonValueKind.String && !string.IsNullOrEmpty(p.GetString()))
                    {
                        list.Add(p.GetString());
                    }
                }
                return list.Count > 0 ? list.ToArray() : null;
            }
        }
    }

    public abstract class FileDialog : CommonDialog
    {
        public string FileName { get; set; } = "";
        public string[] FileNames { get; private set; } = Array.Empty<string>();
        public string Filter { get; set; } = "";
        public int FilterIndex { get; set; } = 1;
        public string Title { get; set; } = "";
        public string InitialDirectory { get; set; } = "";
        public string DefaultExt { get; set; } = "";
        public bool AddExtension { get; set; } = true;
        public bool CheckFileExists { get; set; }
        public bool CheckPathExists { get; set; } = true;
        public bool RestoreDirectory { get; set; }
        public bool ValidateNames { get; set; } = true;
        internal abstract string Mode { get; }
        internal virtual bool Multi => false;

        protected override bool RunDialog()
        {
            var paths = Ask(Mode, Title, Filter, InitialDirectory, FileName, Multi);
            if (paths == null)
            {
                return false;
            }
            if (Mode == "save" && AddExtension && !string.IsNullOrEmpty(DefaultExt) && string.IsNullOrEmpty(Path.GetExtension(paths[0])))
            {
                paths[0] = paths[0] + "." + DefaultExt.TrimStart('.');
            }
            FileNames = paths;
            FileName = paths[0];
            return true;
        }
    }

    public sealed class OpenFileDialog : FileDialog
    {
        public OpenFileDialog() { CheckFileExists = true; }
        public bool Multiselect { get; set; }
        public bool ReadOnlyChecked { get; set; }
        internal override string Mode => "open";
        internal override bool Multi => Multiselect;
        public Stream OpenFile() => File.OpenRead(FileName);
    }

    public sealed class SaveFileDialog : FileDialog
    {
        public bool OverwritePrompt { get; set; } = true;
        public bool CreatePrompt { get; set; }
        internal override string Mode => "save";
        public Stream OpenFile() => File.Open(FileName, FileMode.Create, FileAccess.ReadWrite);
    }

    public sealed class FolderBrowserDialog : CommonDialog
    {
        public string SelectedPath { get; set; } = "";
        public string Description { get; set; } = "";
        public bool ShowNewFolderButton { get; set; } = true;
        public bool UseDescriptionForTitle { get; set; }
        public Environment.SpecialFolder RootFolder { get; set; } = Environment.SpecialFolder.Desktop;

        protected override bool RunDialog()
        {
            var paths = Ask("folder", Description, "", SelectedPath, "", false);
            if (paths == null)
            {
                return false;
            }
            SelectedPath = paths[0];
            return true;
        }
    }
}
"#;
