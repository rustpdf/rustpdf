{ Idiomatic Object Pascal (Delphi / Free Pascal) binding for the rust-pdf core
  over its C ABI (libpdf_ffi). Covers the whole product surface: vector
  graphics, fonts/text, paragraphs, images, PDF/A (1b-3a), tagged/accessible
  output, attachments, AcroForm fields, manipulation, text extraction,
  encryption and digital signatures, plus feature licensing.

  Pure FFI: the cdylib is located at run time (RUSTPDF_LIB or the workspace
  target/debug or target/release) and bound dynamically -- no static link name, so
  the same unit works from the repo and from an installed library path. The C
  declarations mirror include/pdf.h; regenerate that header with `make header`.

  Compiles under Delphi (10.x+, Win/macOS/Linux) and Free Pascal (3.2+). }
unit RustPdf;

{$IFDEF FPC}
  {$MODE DELPHI}
  {$H+}
  {$CODEPAGE UTF8}
{$ENDIF}

interface

uses
  SysUtils;

type
  { PdfStatus codes returned by every fallible export (see include/pdf.h). }
  TPdfStatus = (
    psOk              = 0,
    psNullPointer     = 1,
    psInvalidUtf8     = 2,
    psIo              = 3,
    psSerialize       = 4,
    psPanic           = 5,
    psParse           = 6,
    psFont            = 7,
    psImage           = 8,
    psEncrypt         = 9,
    psSign            = 10,
    psInvalidArgument = 11,
    psLicense         = 12
  );

  { PDF/A conformance level (argument to TPdfDocument.Pdfa). }
  TPdfaLevel = (palA1B, palA2B, palA2A, palA3B, palA3A);

  { Paragraph alignment. }
  TPdfAlign = (paLeft, paRight, paCenter, paJustify);

  { Embedded-file relationship (PDF/A-3 attachments). }
  TAFRelationship = (afSource, afData, afAlternative, afSupplement, afUnspecified);

  { Encryption cipher (TPdfEditable.Encrypt). }
  TEncryption = (encRC4_128, encAES128, encAES256);

  { Axis-aligned rectangle [x0,y0,x1,y1] for AcroForm widgets. }
  TPdfRect = record
    X0, Y0, X1, Y1: Double;
  end;

  { One radio button: its widget rectangle plus its /AP export value. }
  TRadioButton = record
    Rect: TPdfRect;
    ExportValue: string;
  end;
  TRadioButtons = array of TRadioButton;

  { Raised when a native call returns a non-zero PdfStatus, or when the library
    cannot be located/loaded. }
  ERustPdf = class(Exception)
  private
    FStatus: TPdfStatus;
  public
    constructor Create(const Msg: string; AStatus: TPdfStatus);
    property Status: TPdfStatus read FStatus;
  end;

  { A PDF document being authored. Free with .Free (Owned via TObject) -- the
    destructor releases the native handle. Most mutators return Self so calls
    can be chained. }
  TPdfDocument = class(TObject)
  private
    FHandle: Pointer;
    function H: Pointer;
  public
    constructor Create;
    destructor Destroy; override;

    { ---- configuration ---- }
    function Pdfa: TPdfDocument; overload;                 // PDF/A-2b
    function Pdfa(Level: TPdfaLevel): TPdfDocument; overload;
    function Tagged: TPdfDocument;
    function SetVersion(V: Integer): TPdfDocument;         // 0=1.4, 1=1.5, 2=1.7
    function DefaultSize(Width, Height: Double): TPdfDocument;
    function SetInfo(const Title, Author, Subject, Keywords, Creator: string): TPdfDocument;

    { ---- pages + graphics ---- }
    function AddPage: TPdfDocument; overload;
    function AddPage(Width, Height: Double): TPdfDocument; overload;
    function FillRgb(R, G, B: Double): TPdfDocument;
    function StrokeRgb(R, G, B: Double): TPdfDocument;
    function LineWidth(W: Double): TPdfDocument;
    function Rect(X, Y, W, H: Double): TPdfDocument;
    function Fill: TPdfDocument;
    function Stroke: TPdfDocument;

    { ---- fonts + text ---- }
    function AddFontFile(const Path: string): Integer;
    function AddFont(const Data: TBytes): Integer;
    function ShowText(Font: Integer; Size, X, Y: Double; const Text: string;
                      HeadingLevel: Integer = 0): TPdfDocument;
    function Paragraph(Font: Integer; Size, X, Y, Width: Double; const Text: string;
                       Align: TPdfAlign = paLeft): TPdfDocument;

    { ---- images ---- }
    function AddImageFile(const Path: string): Integer;
    function AddImagePng(const Data: TBytes): Integer;
    function AddImageJpeg(const Data: TBytes): Integer;
    function DrawImage(Image: Integer; X, Y, W, H: Double): TPdfDocument;
    function Figure(Image: Integer; X, Y, W, H: Double; const Alt: string): TPdfDocument;

    { ---- attachments + forms ---- }
    function AttachFile(const Name, Mime: string; const Data: TBytes;
                        Relationship: TAFRelationship = afSource;
                        const Description: string = ''): TPdfDocument;
    function TextField(const Name: string; Page: NativeUInt; const R: TPdfRect;
                       const Value: string = ''; Size: Double = 0.0): TPdfDocument;
    function Checkbox(const Name: string; Page: NativeUInt; const R: TPdfRect;
                      Checked: Boolean): TPdfDocument;
    function Dropdown(const Name: string; Page: NativeUInt; const R: TPdfRect;
                      const Options: array of string; Selected: Integer = -1;
                      Size: Double = 0.0): TPdfDocument;
    function RadioGroup(const Name: string; Page: NativeUInt;
                        const Buttons: TRadioButtons; Selected: Integer = -1): TPdfDocument;

    { ---- output ---- }
    function PageCount: Integer;
    function ToBytes: TBytes;
    procedure SaveToFile(const Path: string);

    property Handle: Pointer read FHandle;
  end;

  { An existing PDF loaded for manipulation. Create via Load / LoadFromFile. }
  TPdfEditable = class(TObject)
  private
    FHandle: Pointer;
    function H: Pointer;
    constructor CreateFromHandle(AHandle: Pointer);
  public
    class function Load(const Data: TBytes): TPdfEditable; overload;
    class function Load(const Data: TBytes; const Password: string): TPdfEditable; overload;
    class function LoadFromFile(const Path: string): TPdfEditable; overload;
    class function LoadFromFile(const Path, Password: string): TPdfEditable; overload;
    destructor Destroy; override;

    function PageCount: Integer;
    function Merge(Other: TPdfEditable): TPdfEditable;
    function RotatePage(Index: NativeUInt; Degrees: Integer): TPdfEditable;
    function DeletePage(Index: NativeUInt): TPdfEditable;
    function ReorderPages(const Order: array of NativeUInt): TPdfEditable;
    function ExtractPages(const Indices: array of NativeUInt): TPdfEditable;
    function SetInfo(const Key, Value: string): TPdfEditable;
    function GetInfo(const Key: string): string;
    function SetXmp(const Xml: TBytes): TPdfEditable;
    function OverlayPage(Index: NativeUInt; const Content: TBytes): TPdfEditable;
    function FillTextField(const Name, Value: string): Boolean;
    function Optimize: TPdfEditable;
    function Compact(On: Boolean = True): TPdfEditable;
    function Encrypt(Method: TEncryption = encAES256; const User: string = '';
                     const Owner: string = ''; ReadOnlyPerms: Boolean = False): TPdfEditable;
    function ToBytes: TBytes;
    function ToBytesIncremental(const Original: TBytes): TBytes;
    procedure SaveToFile(const Path: string);

    property Handle: Pointer read FHandle;
  end;

  { Stateless package-level entry points (version, licensing, extraction,
    signing). A record with class methods, used as `Pdf.Sign(...)`. }
  Pdf = record
    class function Version: string; static;
    class procedure ActivateLicense(const Token: string); static;
    class function ExtractText(const PdfBytes: TBytes): string; static;
    class function Sign(const PdfBytes, KeyDer, CertDer: TBytes;
                        const Reason: string = ''; const Location: string = '';
                        const Name: string = ''; Pades: Boolean = False): TBytes; static;
    class function Timestamp(const PdfBytes, TsaKeyDer, TsaCertDer: TBytes;
                             const Date: string = ''): TBytes; static;
    class function AddDss(const PdfBytes: TBytes;
                         const Certs: array of TBytes;
                         const Crls: array of TBytes): TBytes; static;
  end;

{ Convenience constructor for a TPdfRect. }
function PdfRect(X0, Y0, X1, Y1: Double): TPdfRect;

implementation

uses
  Classes,
  {$IFDEF FPC}
  dynlibs
  {$ELSE}
    {$IFDEF MSWINDOWS}
    Winapi.Windows
    {$ELSE}
    Posix.Dlfcn
    {$ENDIF}
  {$ENDIF}
  ;

{ ===================== dynamic-library shim ============================== }

type
  {$IFDEF FPC}
  TLibH = TLibHandle;
  {$ELSE}
    {$IFDEF MSWINDOWS}
  TLibH = HMODULE;
    {$ELSE}
  TLibH = Pointer;
    {$ENDIF}
  {$ENDIF}

function LlOpen(const Name: string): TLibH;
begin
  {$IFDEF FPC}
  Result := LoadLibrary(Name);
  {$ELSE}
    {$IFDEF MSWINDOWS}
  Result := LoadLibrary(PChar(Name));
    {$ELSE}
  Result := dlopen(PAnsiChar(AnsiString(Name)), RTLD_NOW);
    {$ENDIF}
  {$ENDIF}
end;

function LlSym(Lib: TLibH; const Name: string): Pointer;
begin
  {$IFDEF FPC}
  Result := GetProcedureAddress(Lib, Name);
  {$ELSE}
    {$IFDEF MSWINDOWS}
  Result := GetProcAddress(Lib, PChar(Name));
    {$ELSE}
  Result := dlsym(Lib, PAnsiChar(AnsiString(Name)));
    {$ENDIF}
  {$ENDIF}
end;

{ ===================== raw C function pointer types ===================== }

type
  Tpdf_version              = function(): PAnsiChar; cdecl;
  Tpdf_last_error_message   = function(): PAnsiChar; cdecl;
  Tpdf_activate_license     = function(token: PAnsiChar): Integer; cdecl;
  Tpdf_buffer_free          = procedure(ptr: PByte; len: NativeUInt); cdecl;

  Tpdf_document_new         = function(): Pointer; cdecl;
  Tpdf_document_free        = procedure(doc: Pointer); cdecl;
  Tpdf_document_add_page    = function(doc: Pointer): Integer; cdecl;
  Tpdf_document_add_page_sized = function(doc: Pointer; w, h: Double): Integer; cdecl;
  Tpdf_document_page_count  = function(doc: Pointer): Integer; cdecl;
  Tpdf_page_set_fill_rgb    = function(doc: Pointer; r, g, b: Double): Integer; cdecl;
  Tpdf_page_set_stroke_rgb  = function(doc: Pointer; r, g, b: Double): Integer; cdecl;
  Tpdf_page_set_line_width  = function(doc: Pointer; w: Double): Integer; cdecl;
  Tpdf_page_rect            = function(doc: Pointer; x, y, w, h: Double): Integer; cdecl;
  Tpdf_page_fill            = function(doc: Pointer): Integer; cdecl;
  Tpdf_page_stroke          = function(doc: Pointer): Integer; cdecl;
  Tpdf_document_save        = function(doc: Pointer; path: PAnsiChar): Integer; cdecl;
  Tpdf_document_write       = function(doc: Pointer; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_document_pdfa        = function(doc: Pointer): Integer; cdecl;
  Tpdf_document_pdfa_level  = function(doc: Pointer; level: Integer): Integer; cdecl;
  Tpdf_document_tagged      = function(doc: Pointer): Integer; cdecl;
  Tpdf_document_set_version = function(doc: Pointer; v: Integer): Integer; cdecl;
  Tpdf_document_set_default_size = function(doc: Pointer; w, h: Double): Integer; cdecl;
  Tpdf_document_set_info    = function(doc: Pointer; title, author, subject, keywords, creator: PAnsiChar): Integer; cdecl;
  Tpdf_document_add_font_file = function(doc: Pointer; path: PAnsiChar; out outid: Integer): Integer; cdecl;
  Tpdf_document_add_font    = function(doc: Pointer; data: PByte; len: NativeUInt; out outid: Integer): Integer; cdecl;
  Tpdf_page_show_text       = function(doc: Pointer; font: Integer; size, x, y: Double; text: PAnsiChar; heading: Integer): Integer; cdecl;
  Tpdf_page_paragraph       = function(doc: Pointer; font: Integer; size, x, y, width: Double; align: Integer; text: PAnsiChar): Integer; cdecl;
  Tpdf_document_add_image_file = function(doc: Pointer; path: PAnsiChar; out outid: Integer): Integer; cdecl;
  Tpdf_document_add_image_png  = function(doc: Pointer; data: PByte; len: NativeUInt; out outid: Integer): Integer; cdecl;
  Tpdf_document_add_image_jpeg = function(doc: Pointer; data: PByte; len: NativeUInt; out outid: Integer): Integer; cdecl;
  Tpdf_page_draw_image      = function(doc: Pointer; image: Integer; x, y, w, h: Double): Integer; cdecl;
  Tpdf_page_figure          = function(doc: Pointer; image: Integer; x, y, w, h: Double; alt: PAnsiChar): Integer; cdecl;
  Tpdf_document_attach_file = function(doc: Pointer; name, mime: PAnsiChar; data: PByte; len: NativeUInt; relationship: Integer; desc: PAnsiChar): Integer; cdecl;
  Tpdf_document_text_field  = function(doc: Pointer; name: PAnsiChar; page: NativeUInt; x0, y0, x1, y1: Double; value: PAnsiChar; size: Double): Integer; cdecl;
  Tpdf_document_checkbox    = function(doc: Pointer; name: PAnsiChar; page: NativeUInt; x0, y0, x1, y1: Double; checked: Integer): Integer; cdecl;
  Tpdf_document_dropdown    = function(doc: Pointer; name: PAnsiChar; page: NativeUInt; x0, y0, x1, y1: Double; options: PAnsiChar; selected: Integer; size: Double): Integer; cdecl;
  Tpdf_document_radio_group = function(doc: Pointer; name: PAnsiChar; page, count: NativeUInt; rects: Pointer; exportsArr: Pointer; selected: Integer): Integer; cdecl;

  Tpdf_editable_load        = function(data: PByte; len: NativeUInt): Pointer; cdecl;
  Tpdf_editable_load_password = function(data: PByte; len: NativeUInt; password: PAnsiChar): Pointer; cdecl;
  Tpdf_editable_free        = procedure(ed: Pointer); cdecl;
  Tpdf_editable_page_count  = function(ed: Pointer): Integer; cdecl;
  Tpdf_editable_merge       = function(ed, other: Pointer): Integer; cdecl;
  Tpdf_editable_rotate_page = function(ed: Pointer; index: NativeUInt; degrees: Integer): Integer; cdecl;
  Tpdf_editable_delete_page = function(ed: Pointer; index: NativeUInt): Integer; cdecl;
  Tpdf_editable_reorder_pages = function(ed: Pointer; order: Pointer; count: NativeUInt): Integer; cdecl;
  Tpdf_editable_extract_pages = function(ed: Pointer; indices: Pointer; count: NativeUInt; out outed: Pointer): Integer; cdecl;
  Tpdf_editable_set_info    = function(ed: Pointer; key, value: PAnsiChar): Integer; cdecl;
  Tpdf_editable_get_info    = function(ed: Pointer; key: PAnsiChar; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_editable_set_xmp     = function(ed: Pointer; xml: PByte; len: NativeUInt): Integer; cdecl;
  Tpdf_editable_overlay_page = function(ed: Pointer; index: NativeUInt; content: PByte; len: NativeUInt): Integer; cdecl;
  Tpdf_editable_fill_text_field = function(ed: Pointer; name, value: PAnsiChar; out outfound: Integer): Integer; cdecl;
  Tpdf_editable_optimize    = function(ed: Pointer): Integer; cdecl;
  Tpdf_editable_compact     = function(ed: Pointer; on_: Integer): Integer; cdecl;
  Tpdf_editable_encrypt     = function(ed: Pointer; method: Integer; user, owner: PAnsiChar; read_only: Integer): Integer; cdecl;
  Tpdf_editable_to_bytes    = function(ed: Pointer; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_editable_to_bytes_incremental = function(ed: Pointer; original: PByte; original_len: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_editable_save        = function(ed: Pointer; path: PAnsiChar): Integer; cdecl;

  Tpdf_extract_text         = function(data: PByte; len: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_sign                 = function(pdf: PByte; pdf_len: NativeUInt; key: PByte; key_len: NativeUInt; cert: PByte; cert_len: NativeUInt; reason, location, name: PAnsiChar; pades: Integer; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_timestamp            = function(pdf: PByte; pdf_len: NativeUInt; key: PByte; key_len: NativeUInt; cert: PByte; cert_len: NativeUInt; date: PAnsiChar; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_add_dss              = function(pdf: PByte; pdf_len: NativeUInt; cert_ptrs: Pointer; cert_lens: Pointer; cert_count: NativeUInt; crl_ptrs: Pointer; crl_lens: Pointer; crl_count: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;

{ ===================== bound function table ============================= }

var
  GLib: TLibH;
  GLoaded: Boolean = False;

  Fpdf_version: Tpdf_version;
  Fpdf_last_error_message: Tpdf_last_error_message;
  Fpdf_activate_license: Tpdf_activate_license;
  Fpdf_buffer_free: Tpdf_buffer_free;
  Fpdf_document_new: Tpdf_document_new;
  Fpdf_document_free: Tpdf_document_free;
  Fpdf_document_add_page: Tpdf_document_add_page;
  Fpdf_document_add_page_sized: Tpdf_document_add_page_sized;
  Fpdf_document_page_count: Tpdf_document_page_count;
  Fpdf_page_set_fill_rgb: Tpdf_page_set_fill_rgb;
  Fpdf_page_set_stroke_rgb: Tpdf_page_set_stroke_rgb;
  Fpdf_page_set_line_width: Tpdf_page_set_line_width;
  Fpdf_page_rect: Tpdf_page_rect;
  Fpdf_page_fill: Tpdf_page_fill;
  Fpdf_page_stroke: Tpdf_page_stroke;
  Fpdf_document_save: Tpdf_document_save;
  Fpdf_document_write: Tpdf_document_write;
  Fpdf_document_pdfa: Tpdf_document_pdfa;
  Fpdf_document_pdfa_level: Tpdf_document_pdfa_level;
  Fpdf_document_tagged: Tpdf_document_tagged;
  Fpdf_document_set_version: Tpdf_document_set_version;
  Fpdf_document_set_default_size: Tpdf_document_set_default_size;
  Fpdf_document_set_info: Tpdf_document_set_info;
  Fpdf_document_add_font_file: Tpdf_document_add_font_file;
  Fpdf_document_add_font: Tpdf_document_add_font;
  Fpdf_page_show_text: Tpdf_page_show_text;
  Fpdf_page_paragraph: Tpdf_page_paragraph;
  Fpdf_document_add_image_file: Tpdf_document_add_image_file;
  Fpdf_document_add_image_png: Tpdf_document_add_image_png;
  Fpdf_document_add_image_jpeg: Tpdf_document_add_image_jpeg;
  Fpdf_page_draw_image: Tpdf_page_draw_image;
  Fpdf_page_figure: Tpdf_page_figure;
  Fpdf_document_attach_file: Tpdf_document_attach_file;
  Fpdf_document_text_field: Tpdf_document_text_field;
  Fpdf_document_checkbox: Tpdf_document_checkbox;
  Fpdf_document_dropdown: Tpdf_document_dropdown;
  Fpdf_document_radio_group: Tpdf_document_radio_group;
  Fpdf_editable_load: Tpdf_editable_load;
  Fpdf_editable_load_password: Tpdf_editable_load_password;
  Fpdf_editable_free: Tpdf_editable_free;
  Fpdf_editable_page_count: Tpdf_editable_page_count;
  Fpdf_editable_merge: Tpdf_editable_merge;
  Fpdf_editable_rotate_page: Tpdf_editable_rotate_page;
  Fpdf_editable_delete_page: Tpdf_editable_delete_page;
  Fpdf_editable_reorder_pages: Tpdf_editable_reorder_pages;
  Fpdf_editable_extract_pages: Tpdf_editable_extract_pages;
  Fpdf_editable_set_info: Tpdf_editable_set_info;
  Fpdf_editable_get_info: Tpdf_editable_get_info;
  Fpdf_editable_set_xmp: Tpdf_editable_set_xmp;
  Fpdf_editable_overlay_page: Tpdf_editable_overlay_page;
  Fpdf_editable_fill_text_field: Tpdf_editable_fill_text_field;
  Fpdf_editable_optimize: Tpdf_editable_optimize;
  Fpdf_editable_compact: Tpdf_editable_compact;
  Fpdf_editable_encrypt: Tpdf_editable_encrypt;
  Fpdf_editable_to_bytes: Tpdf_editable_to_bytes;
  Fpdf_editable_to_bytes_incremental: Tpdf_editable_to_bytes_incremental;
  Fpdf_editable_save: Tpdf_editable_save;
  Fpdf_extract_text: Tpdf_extract_text;
  Fpdf_sign: Tpdf_sign;
  Fpdf_timestamp: Tpdf_timestamp;
  Fpdf_add_dss: Tpdf_add_dss;

{ ===================== loader ========================================== }

function LibFileName: string;
begin
  {$IFDEF MSWINDOWS}
  Result := 'pdf_ffi.dll';
  {$ELSE}
    {$IFDEF DARWIN}
  Result := 'libpdf_ffi.dylib';
    {$ELSE}
      {$IFDEF MACOS}
  Result := 'libpdf_ffi.dylib';
      {$ELSE}
  Result := 'libpdf_ffi.so';
      {$ENDIF}
    {$ENDIF}
  {$ENDIF}
end;

{ Find the cdylib, in order:
    1. $RUSTPDF_LIB (an explicit path);
    2. the native library sitting next to the executable or in the current dir
       (the normal deployment layout — ship the lib beside your app);
    3. target/debug|release/<libfile> walking up from those dirs (the dev tree).
  If all miss, the caller falls back to the bare platform name and lets the OS
  loader resolve it (install-name / PATH / LD_LIBRARY_PATH). Mirrors the
  resolver used by the C#, Java and Ruby bindings. }
function ResolveLibPath: string;
var
  Env, FileName, Dir, Parent, Candidate: string;
  Profiles: array[0..1] of string;
  Roots: array[0..1] of string;
  I, P, R: Integer;
begin
  Env := GetEnvironmentVariable('RUSTPDF_LIB');
  if (Env <> '') and FileExists(Env) then
    Exit(Env);

  FileName := LibFileName;
  Profiles[0] := 'debug';
  Profiles[1] := 'release';
  Roots[0] := ExtractFileDir(ParamStr(0));
  Roots[1] := GetCurrentDir;

  { Deployment layout: the library is right next to the app or in the CWD. }
  for R := 0 to High(Roots) do
  begin
    if Roots[R] = '' then
      Continue;
    Candidate := IncludeTrailingPathDelimiter(Roots[R]) + FileName;
    if FileExists(Candidate) then
      Exit(Candidate);
  end;

  for R := 0 to High(Roots) do
  begin
    Dir := Roots[R];
    for I := 0 to 11 do
    begin
      for P := 0 to High(Profiles) do
      begin
        Candidate := IncludeTrailingPathDelimiter(Dir) + 'target' + PathDelim +
                     Profiles[P] + PathDelim + FileName;
        if FileExists(Candidate) then
          Exit(Candidate);
      end;
      Parent := ExtractFileDir(ExcludeTrailingPathDelimiter(Dir));
      if (Parent = '') or (Parent = Dir) then
        Break;
      Dir := Parent;
    end;
  end;

  { Last resort: let the OS loader resolve the bare name (install-name / PATH). }
  Result := FileName;
end;

function Bind(const Name: string): Pointer;
begin
  Result := LlSym(GLib, Name);
  if Result = nil then
    raise ERustPdf.Create('missing export: ' + Name, psNullPointer);
end;

procedure EnsureLoaded;
var
  Path: string;
begin
  if GLoaded then
    Exit;

  Path := ResolveLibPath;
  GLib := LlOpen(Path);
  {$IFDEF FPC}
  if GLib = NilHandle then
  {$ELSE}
    {$IFDEF MSWINDOWS}
  if GLib = 0 then
    {$ELSE}
  if GLib = nil then
    {$ENDIF}
  {$ENDIF}
    raise ERustPdf.Create('could not load ' + Path +
      ' -- build it with `cargo build -p pdf-ffi` or set RUSTPDF_LIB', psIo);

  Fpdf_version := Tpdf_version(Bind('pdf_version'));
  Fpdf_last_error_message := Tpdf_last_error_message(Bind('pdf_last_error_message'));
  Fpdf_activate_license := Tpdf_activate_license(Bind('pdf_activate_license'));
  Fpdf_buffer_free := Tpdf_buffer_free(Bind('pdf_buffer_free'));
  Fpdf_document_new := Tpdf_document_new(Bind('pdf_document_new'));
  Fpdf_document_free := Tpdf_document_free(Bind('pdf_document_free'));
  Fpdf_document_add_page := Tpdf_document_add_page(Bind('pdf_document_add_page'));
  Fpdf_document_add_page_sized := Tpdf_document_add_page_sized(Bind('pdf_document_add_page_sized'));
  Fpdf_document_page_count := Tpdf_document_page_count(Bind('pdf_document_page_count'));
  Fpdf_page_set_fill_rgb := Tpdf_page_set_fill_rgb(Bind('pdf_page_set_fill_rgb'));
  Fpdf_page_set_stroke_rgb := Tpdf_page_set_stroke_rgb(Bind('pdf_page_set_stroke_rgb'));
  Fpdf_page_set_line_width := Tpdf_page_set_line_width(Bind('pdf_page_set_line_width'));
  Fpdf_page_rect := Tpdf_page_rect(Bind('pdf_page_rect'));
  Fpdf_page_fill := Tpdf_page_fill(Bind('pdf_page_fill'));
  Fpdf_page_stroke := Tpdf_page_stroke(Bind('pdf_page_stroke'));
  Fpdf_document_save := Tpdf_document_save(Bind('pdf_document_save'));
  Fpdf_document_write := Tpdf_document_write(Bind('pdf_document_write'));
  Fpdf_document_pdfa := Tpdf_document_pdfa(Bind('pdf_document_pdfa'));
  Fpdf_document_pdfa_level := Tpdf_document_pdfa_level(Bind('pdf_document_pdfa_level'));
  Fpdf_document_tagged := Tpdf_document_tagged(Bind('pdf_document_tagged'));
  Fpdf_document_set_version := Tpdf_document_set_version(Bind('pdf_document_set_version'));
  Fpdf_document_set_default_size := Tpdf_document_set_default_size(Bind('pdf_document_set_default_size'));
  Fpdf_document_set_info := Tpdf_document_set_info(Bind('pdf_document_set_info'));
  Fpdf_document_add_font_file := Tpdf_document_add_font_file(Bind('pdf_document_add_font_file'));
  Fpdf_document_add_font := Tpdf_document_add_font(Bind('pdf_document_add_font'));
  Fpdf_page_show_text := Tpdf_page_show_text(Bind('pdf_page_show_text'));
  Fpdf_page_paragraph := Tpdf_page_paragraph(Bind('pdf_page_paragraph'));
  Fpdf_document_add_image_file := Tpdf_document_add_image_file(Bind('pdf_document_add_image_file'));
  Fpdf_document_add_image_png := Tpdf_document_add_image_png(Bind('pdf_document_add_image_png'));
  Fpdf_document_add_image_jpeg := Tpdf_document_add_image_jpeg(Bind('pdf_document_add_image_jpeg'));
  Fpdf_page_draw_image := Tpdf_page_draw_image(Bind('pdf_page_draw_image'));
  Fpdf_page_figure := Tpdf_page_figure(Bind('pdf_page_figure'));
  Fpdf_document_attach_file := Tpdf_document_attach_file(Bind('pdf_document_attach_file'));
  Fpdf_document_text_field := Tpdf_document_text_field(Bind('pdf_document_text_field'));
  Fpdf_document_checkbox := Tpdf_document_checkbox(Bind('pdf_document_checkbox'));
  Fpdf_document_dropdown := Tpdf_document_dropdown(Bind('pdf_document_dropdown'));
  Fpdf_document_radio_group := Tpdf_document_radio_group(Bind('pdf_document_radio_group'));
  Fpdf_editable_load := Tpdf_editable_load(Bind('pdf_editable_load'));
  Fpdf_editable_load_password := Tpdf_editable_load_password(Bind('pdf_editable_load_password'));
  Fpdf_editable_free := Tpdf_editable_free(Bind('pdf_editable_free'));
  Fpdf_editable_page_count := Tpdf_editable_page_count(Bind('pdf_editable_page_count'));
  Fpdf_editable_merge := Tpdf_editable_merge(Bind('pdf_editable_merge'));
  Fpdf_editable_rotate_page := Tpdf_editable_rotate_page(Bind('pdf_editable_rotate_page'));
  Fpdf_editable_delete_page := Tpdf_editable_delete_page(Bind('pdf_editable_delete_page'));
  Fpdf_editable_reorder_pages := Tpdf_editable_reorder_pages(Bind('pdf_editable_reorder_pages'));
  Fpdf_editable_extract_pages := Tpdf_editable_extract_pages(Bind('pdf_editable_extract_pages'));
  Fpdf_editable_set_info := Tpdf_editable_set_info(Bind('pdf_editable_set_info'));
  Fpdf_editable_get_info := Tpdf_editable_get_info(Bind('pdf_editable_get_info'));
  Fpdf_editable_set_xmp := Tpdf_editable_set_xmp(Bind('pdf_editable_set_xmp'));
  Fpdf_editable_overlay_page := Tpdf_editable_overlay_page(Bind('pdf_editable_overlay_page'));
  Fpdf_editable_fill_text_field := Tpdf_editable_fill_text_field(Bind('pdf_editable_fill_text_field'));
  Fpdf_editable_optimize := Tpdf_editable_optimize(Bind('pdf_editable_optimize'));
  Fpdf_editable_compact := Tpdf_editable_compact(Bind('pdf_editable_compact'));
  Fpdf_editable_encrypt := Tpdf_editable_encrypt(Bind('pdf_editable_encrypt'));
  Fpdf_editable_to_bytes := Tpdf_editable_to_bytes(Bind('pdf_editable_to_bytes'));
  Fpdf_editable_to_bytes_incremental := Tpdf_editable_to_bytes_incremental(Bind('pdf_editable_to_bytes_incremental'));
  Fpdf_editable_save := Tpdf_editable_save(Bind('pdf_editable_save'));
  Fpdf_extract_text := Tpdf_extract_text(Bind('pdf_extract_text'));
  Fpdf_sign := Tpdf_sign(Bind('pdf_sign'));
  Fpdf_timestamp := Tpdf_timestamp(Bind('pdf_timestamp'));
  Fpdf_add_dss := Tpdf_add_dss(Bind('pdf_add_dss'));

  GLoaded := True;
end;

{ ===================== helpers ========================================= }

{ Copy a NUL-terminated UTF-8 C string into a Pascal string. Uses assignment
  (PAnsiChar -> AnsiString copies the bytes); a value typecast would merely
  reinterpret the pointer. }
function PAnsiToString(P: PAnsiChar): string;
var
  U: UTF8String;
begin
  if P = nil then
    Exit('');
  U := P;
  Result := string(U);
end;

function LastError: string;
var
  P: PAnsiChar;
begin
  P := Fpdf_last_error_message();
  if P = nil then
    Result := 'unknown error'
  else
    Result := PAnsiToString(P);
end;

procedure Check(Status: Integer);
begin
  if Status <> 0 then
    raise ERustPdf.Create(LastError, TPdfStatus(Status));
end;

{ UTF-8 view of a Pascal string, kept alive by the caller's local var. }
function U8(const S: string): UTF8String; inline;
begin
  Result := UTF8String(S);
end;

{ Pointer to a UTF-8 buffer, or nil for an empty string (NULL = "leave unset"
  for the optional /Info arguments). }
function OptU8(const U: UTF8String; const Orig: string): PAnsiChar; inline;
begin
  if Orig = '' then
    Result := nil
  else
    Result := PAnsiChar(U);
end;

{ Pointer to the first byte of a TBytes (nil for an empty array). }
function BytePtr(const B: TBytes): PByte; inline;
begin
  if Length(B) = 0 then
    Result := nil
  else
    Result := @B[0];
end;

{ Pointers to the storage of open/dynamic arrays, for the array-valued C
  parameters (nil when empty). Open-array params reference the caller's memory,
  so the address stays valid for the duration of the native call. }
function DblPtr(const A: array of Double): Pointer; inline;
begin
  if Length(A) = 0 then Result := nil else Result := @A[0];
end;

function SzPtr(const A: array of NativeUInt): Pointer; inline;
begin
  if Length(A) = 0 then Result := nil else Result := @A[0];
end;

function StrPtr(const A: array of PAnsiChar): Pointer; inline;
begin
  if Length(A) = 0 then Result := nil else Result := @A[0];
end;

function BPtr(const A: array of PByte): Pointer; inline;
begin
  if Length(A) = 0 then Result := nil else Result := @A[0];
end;

{ Read a whole file into a TBytes (used by TPdfEditable.LoadFromFile). }
function ReadFileBytes(const Path: string): TBytes;
var
  Stream: TFileStream;
begin
  Stream := TFileStream.Create(Path, fmOpenRead or fmShareDenyWrite);
  try
    SetLength(Result, Stream.Size);
    if Stream.Size > 0 then
      Stream.ReadBuffer(Result[0], Stream.Size);
  finally
    Stream.Free;
  end;
end;

{ Copy a native out-buffer into a TBytes, then release it with pdf_buffer_free. }
function TakeBuffer(P: PByte; Len: NativeUInt): TBytes;
begin
  SetLength(Result, Len);
  if (Len > 0) and (P <> nil) then
    Move(P^, Result[0], Len);
  if P <> nil then
    Fpdf_buffer_free(P, Len);
end;

function Utf8BytesToString(const B: TBytes): string;
var
  U: UTF8String;
begin
  SetLength(U, Length(B));
  if Length(B) > 0 then
    Move(B[0], U[1], Length(B));
  Result := string(U);
end;

{ ===================== ERustPdf ======================================== }

constructor ERustPdf.Create(const Msg: string; AStatus: TPdfStatus);
begin
  FStatus := AStatus;
  if AStatus = psOk then
    inherited Create(Msg)
  else
    inherited Create(Format('PdfStatus=%d: %s', [Ord(AStatus), Msg]));
end;

{ ===================== PdfRect ========================================= }

function PdfRect(X0, Y0, X1, Y1: Double): TPdfRect;
begin
  Result.X0 := X0;
  Result.Y0 := Y0;
  Result.X1 := X1;
  Result.Y1 := Y1;
end;

{ ===================== TPdfDocument ==================================== }

constructor TPdfDocument.Create;
begin
  inherited Create;
  EnsureLoaded;
  FHandle := Fpdf_document_new();
  if FHandle = nil then
    raise ERustPdf.Create('pdf_document_new returned NULL', psNullPointer);
end;

destructor TPdfDocument.Destroy;
begin
  if FHandle <> nil then
  begin
    Fpdf_document_free(FHandle);
    FHandle := nil;
  end;
  inherited Destroy;
end;

function TPdfDocument.H: Pointer;
begin
  if FHandle = nil then
    raise ERustPdf.Create('operation on a freed TPdfDocument', psNullPointer);
  Result := FHandle;
end;

function TPdfDocument.Pdfa: TPdfDocument;
begin
  Check(Fpdf_document_pdfa(H));
  Result := Self;
end;

function TPdfDocument.Pdfa(Level: TPdfaLevel): TPdfDocument;
begin
  Check(Fpdf_document_pdfa_level(H, Ord(Level)));
  Result := Self;
end;

function TPdfDocument.Tagged: TPdfDocument;
begin
  Check(Fpdf_document_tagged(H));
  Result := Self;
end;

function TPdfDocument.SetVersion(V: Integer): TPdfDocument;
begin
  Check(Fpdf_document_set_version(H, V));
  Result := Self;
end;

function TPdfDocument.DefaultSize(Width, Height: Double): TPdfDocument;
begin
  Check(Fpdf_document_set_default_size(H, Width, Height));
  Result := Self;
end;

function TPdfDocument.SetInfo(const Title, Author, Subject, Keywords, Creator: string): TPdfDocument;
var
  ut, ua, us, uk, uc: UTF8String;
begin
  ut := U8(Title); ua := U8(Author); us := U8(Subject);
  uk := U8(Keywords); uc := U8(Creator);
  Check(Fpdf_document_set_info(H, OptU8(ut, Title), OptU8(ua, Author),
        OptU8(us, Subject), OptU8(uk, Keywords), OptU8(uc, Creator)));
  Result := Self;
end;

function TPdfDocument.AddPage: TPdfDocument;
begin
  Check(Fpdf_document_add_page(H));
  Result := Self;
end;

function TPdfDocument.AddPage(Width, Height: Double): TPdfDocument;
begin
  Check(Fpdf_document_add_page_sized(H, Width, Height));
  Result := Self;
end;

function TPdfDocument.FillRgb(R, G, B: Double): TPdfDocument;
begin
  Check(Fpdf_page_set_fill_rgb(H, R, G, B));
  Result := Self;
end;

function TPdfDocument.StrokeRgb(R, G, B: Double): TPdfDocument;
begin
  Check(Fpdf_page_set_stroke_rgb(H, R, G, B));
  Result := Self;
end;

function TPdfDocument.LineWidth(W: Double): TPdfDocument;
begin
  Check(Fpdf_page_set_line_width(H, W));
  Result := Self;
end;

function TPdfDocument.Rect(X, Y, W, H: Double): TPdfDocument;
begin
  Check(Fpdf_page_rect(Self.H, X, Y, W, H));
  Result := Self;
end;

function TPdfDocument.Fill: TPdfDocument;
begin
  Check(Fpdf_page_fill(H));
  Result := Self;
end;

function TPdfDocument.Stroke: TPdfDocument;
begin
  Check(Fpdf_page_stroke(H));
  Result := Self;
end;

function TPdfDocument.AddFontFile(const Path: string): Integer;
var
  up: UTF8String;
  id: Integer;
begin
  up := U8(Path);
  Check(Fpdf_document_add_font_file(H, PAnsiChar(up), id));
  Result := id;
end;

function TPdfDocument.AddFont(const Data: TBytes): Integer;
var
  id: Integer;
begin
  Check(Fpdf_document_add_font(H, BytePtr(Data), Length(Data), id));
  Result := id;
end;

function TPdfDocument.ShowText(Font: Integer; Size, X, Y: Double; const Text: string;
  HeadingLevel: Integer): TPdfDocument;
var
  ut: UTF8String;
begin
  ut := U8(Text);
  Check(Fpdf_page_show_text(H, Font, Size, X, Y, PAnsiChar(ut), HeadingLevel));
  Result := Self;
end;

function TPdfDocument.Paragraph(Font: Integer; Size, X, Y, Width: Double; const Text: string;
  Align: TPdfAlign): TPdfDocument;
var
  ut: UTF8String;
begin
  ut := U8(Text);
  Check(Fpdf_page_paragraph(H, Font, Size, X, Y, Width, Ord(Align), PAnsiChar(ut)));
  Result := Self;
end;

function TPdfDocument.AddImageFile(const Path: string): Integer;
var
  up: UTF8String;
  id: Integer;
begin
  up := U8(Path);
  Check(Fpdf_document_add_image_file(H, PAnsiChar(up), id));
  Result := id;
end;

function TPdfDocument.AddImagePng(const Data: TBytes): Integer;
var
  id: Integer;
begin
  Check(Fpdf_document_add_image_png(H, BytePtr(Data), Length(Data), id));
  Result := id;
end;

function TPdfDocument.AddImageJpeg(const Data: TBytes): Integer;
var
  id: Integer;
begin
  Check(Fpdf_document_add_image_jpeg(H, BytePtr(Data), Length(Data), id));
  Result := id;
end;

function TPdfDocument.DrawImage(Image: Integer; X, Y, W, H: Double): TPdfDocument;
begin
  Check(Fpdf_page_draw_image(Self.H, Image, X, Y, W, H));
  Result := Self;
end;

function TPdfDocument.Figure(Image: Integer; X, Y, W, H: Double; const Alt: string): TPdfDocument;
var
  ua: UTF8String;
begin
  ua := U8(Alt);
  Check(Fpdf_page_figure(Self.H, Image, X, Y, W, H, PAnsiChar(ua)));
  Result := Self;
end;

function TPdfDocument.AttachFile(const Name, Mime: string; const Data: TBytes;
  Relationship: TAFRelationship; const Description: string): TPdfDocument;
var
  un, um, ud: UTF8String;
begin
  un := U8(Name); um := U8(Mime); ud := U8(Description);
  Check(Fpdf_document_attach_file(H, PAnsiChar(un), PAnsiChar(um), BytePtr(Data),
        Length(Data), Ord(Relationship), PAnsiChar(ud)));
  Result := Self;
end;

function TPdfDocument.TextField(const Name: string; Page: NativeUInt; const R: TPdfRect;
  const Value: string; Size: Double): TPdfDocument;
var
  un, uv: UTF8String;
begin
  un := U8(Name); uv := U8(Value);
  Check(Fpdf_document_text_field(H, PAnsiChar(un), Page, R.X0, R.Y0, R.X1, R.Y1,
        PAnsiChar(uv), Size));
  Result := Self;
end;

function TPdfDocument.Checkbox(const Name: string; Page: NativeUInt; const R: TPdfRect;
  Checked: Boolean): TPdfDocument;
var
  un: UTF8String;
begin
  un := U8(Name);
  Check(Fpdf_document_checkbox(H, PAnsiChar(un), Page, R.X0, R.Y0, R.X1, R.Y1,
        Ord(Checked)));
  Result := Self;
end;

function TPdfDocument.Dropdown(const Name: string; Page: NativeUInt; const R: TPdfRect;
  const Options: array of string; Selected: Integer; Size: Double): TPdfDocument;
var
  un, uo: UTF8String;
  joined: string;
  I: Integer;
begin
  joined := '';
  for I := 0 to High(Options) do
  begin
    if I > 0 then
      joined := joined + #10;
    joined := joined + Options[I];
  end;
  un := U8(Name); uo := U8(joined);
  Check(Fpdf_document_dropdown(H, PAnsiChar(un), Page, R.X0, R.Y0, R.X1, R.Y1,
        PAnsiChar(uo), Selected, Size));
  Result := Self;
end;

function TPdfDocument.RadioGroup(const Name: string; Page: NativeUInt;
  const Buttons: TRadioButtons; Selected: Integer): TPdfDocument;
var
  un: UTF8String;
  rects: array of Double;
  exps: array of UTF8String;
  ptrs: array of PAnsiChar;
  N, I: Integer;
begin
  un := U8(Name);
  N := Length(Buttons);
  SetLength(rects, N * 4);
  SetLength(exps, N);
  SetLength(ptrs, N);
  for I := 0 to N - 1 do
  begin
    rects[I * 4 + 0] := Buttons[I].Rect.X0;
    rects[I * 4 + 1] := Buttons[I].Rect.Y0;
    rects[I * 4 + 2] := Buttons[I].Rect.X1;
    rects[I * 4 + 3] := Buttons[I].Rect.Y1;
    exps[I] := U8(Buttons[I].ExportValue);
    ptrs[I] := PAnsiChar(exps[I]);
  end;
  Check(Fpdf_document_radio_group(H, PAnsiChar(un), Page, N,
        DblPtr(rects), StrPtr(ptrs), Selected));
  Result := Self;
end;

function TPdfDocument.PageCount: Integer;
begin
  Result := Fpdf_document_page_count(H);
end;

function TPdfDocument.ToBytes: TBytes;
var
  P: PByte;
  Len: NativeUInt;
begin
  Check(Fpdf_document_write(H, P, Len));
  Result := TakeBuffer(P, Len);
end;

procedure TPdfDocument.SaveToFile(const Path: string);
var
  up: UTF8String;
begin
  up := U8(Path);
  Check(Fpdf_document_save(H, PAnsiChar(up)));
end;

{ ===================== TPdfEditable ==================================== }

constructor TPdfEditable.CreateFromHandle(AHandle: Pointer);
begin
  inherited Create;
  FHandle := AHandle;
end;

class function TPdfEditable.Load(const Data: TBytes): TPdfEditable;
var
  P: Pointer;
begin
  EnsureLoaded;
  P := Fpdf_editable_load(BytePtr(Data), Length(Data));
  if P = nil then
    raise ERustPdf.Create(LastError, psParse);
  Result := TPdfEditable.CreateFromHandle(P);
end;

class function TPdfEditable.Load(const Data: TBytes; const Password: string): TPdfEditable;
var
  P: Pointer;
  up: UTF8String;
begin
  EnsureLoaded;
  up := U8(Password);
  P := Fpdf_editable_load_password(BytePtr(Data), Length(Data), PAnsiChar(up));
  if P = nil then
    raise ERustPdf.Create(LastError, psParse);
  Result := TPdfEditable.CreateFromHandle(P);
end;

class function TPdfEditable.LoadFromFile(const Path: string): TPdfEditable;
begin
  Result := Load(ReadFileBytes(Path));
end;

class function TPdfEditable.LoadFromFile(const Path, Password: string): TPdfEditable;
begin
  Result := Load(ReadFileBytes(Path), Password);
end;

destructor TPdfEditable.Destroy;
begin
  if FHandle <> nil then
  begin
    Fpdf_editable_free(FHandle);
    FHandle := nil;
  end;
  inherited Destroy;
end;

function TPdfEditable.H: Pointer;
begin
  if FHandle = nil then
    raise ERustPdf.Create('operation on a freed TPdfEditable', psNullPointer);
  Result := FHandle;
end;

function TPdfEditable.PageCount: Integer;
begin
  Result := Fpdf_editable_page_count(H);
end;

function TPdfEditable.Merge(Other: TPdfEditable): TPdfEditable;
begin
  Check(Fpdf_editable_merge(H, Other.H));
  Result := Self;
end;

function TPdfEditable.RotatePage(Index: NativeUInt; Degrees: Integer): TPdfEditable;
begin
  Check(Fpdf_editable_rotate_page(H, Index, Degrees));
  Result := Self;
end;

function TPdfEditable.DeletePage(Index: NativeUInt): TPdfEditable;
begin
  Check(Fpdf_editable_delete_page(H, Index));
  Result := Self;
end;

function TPdfEditable.ReorderPages(const Order: array of NativeUInt): TPdfEditable;
var
  buf: array of NativeUInt;
  I: Integer;
begin
  SetLength(buf, Length(Order));
  for I := 0 to High(Order) do
    buf[I] := Order[I];
  Check(Fpdf_editable_reorder_pages(H, SzPtr(buf), Length(buf)));
  Result := Self;
end;

function TPdfEditable.ExtractPages(const Indices: array of NativeUInt): TPdfEditable;
var
  buf: array of NativeUInt;
  I: Integer;
  outp: Pointer;
begin
  SetLength(buf, Length(Indices));
  for I := 0 to High(Indices) do
    buf[I] := Indices[I];
  outp := nil;
  Check(Fpdf_editable_extract_pages(H, SzPtr(buf), Length(buf), outp));
  if outp = nil then
    raise ERustPdf.Create(LastError, psInvalidArgument);
  Result := TPdfEditable.CreateFromHandle(outp);
end;

function TPdfEditable.SetInfo(const Key, Value: string): TPdfEditable;
var
  uk, uv: UTF8String;
begin
  uk := U8(Key); uv := U8(Value);
  Check(Fpdf_editable_set_info(H, PAnsiChar(uk), PAnsiChar(uv)));
  Result := Self;
end;

function TPdfEditable.GetInfo(const Key: string): string;
var
  uk: UTF8String;
  P: PByte;
  Len: NativeUInt;
begin
  uk := U8(Key);
  Check(Fpdf_editable_get_info(H, PAnsiChar(uk), P, Len));
  Result := Utf8BytesToString(TakeBuffer(P, Len));
end;

function TPdfEditable.SetXmp(const Xml: TBytes): TPdfEditable;
begin
  Check(Fpdf_editable_set_xmp(H, BytePtr(Xml), Length(Xml)));
  Result := Self;
end;

function TPdfEditable.OverlayPage(Index: NativeUInt; const Content: TBytes): TPdfEditable;
begin
  Check(Fpdf_editable_overlay_page(H, Index, BytePtr(Content), Length(Content)));
  Result := Self;
end;

function TPdfEditable.FillTextField(const Name, Value: string): Boolean;
var
  un, uv: UTF8String;
  found: Integer;
begin
  un := U8(Name); uv := U8(Value);
  found := 0;
  Check(Fpdf_editable_fill_text_field(H, PAnsiChar(un), PAnsiChar(uv), found));
  Result := found <> 0;
end;

function TPdfEditable.Optimize: TPdfEditable;
begin
  Check(Fpdf_editable_optimize(H));
  Result := Self;
end;

function TPdfEditable.Compact(On: Boolean): TPdfEditable;
begin
  Check(Fpdf_editable_compact(H, Ord(On)));
  Result := Self;
end;

function TPdfEditable.Encrypt(Method: TEncryption; const User, Owner: string;
  ReadOnlyPerms: Boolean): TPdfEditable;
var
  uu, uo: UTF8String;
begin
  uu := U8(User); uo := U8(Owner);
  Check(Fpdf_editable_encrypt(H, Ord(Method), PAnsiChar(uu), PAnsiChar(uo),
        Ord(ReadOnlyPerms)));
  Result := Self;
end;

function TPdfEditable.ToBytes: TBytes;
var
  P: PByte;
  Len: NativeUInt;
begin
  Check(Fpdf_editable_to_bytes(H, P, Len));
  Result := TakeBuffer(P, Len);
end;

function TPdfEditable.ToBytesIncremental(const Original: TBytes): TBytes;
var
  P: PByte;
  Len: NativeUInt;
begin
  Check(Fpdf_editable_to_bytes_incremental(H, BytePtr(Original), Length(Original), P, Len));
  Result := TakeBuffer(P, Len);
end;

procedure TPdfEditable.SaveToFile(const Path: string);
var
  up: UTF8String;
begin
  up := U8(Path);
  Check(Fpdf_editable_save(H, PAnsiChar(up)));
end;

{ ===================== Pdf (package functions) ========================= }

class function Pdf.Version: string;
begin
  EnsureLoaded;
  Result := PAnsiToString(Fpdf_version());
end;

class procedure Pdf.ActivateLicense(const Token: string);
var
  ut: UTF8String;
begin
  EnsureLoaded;
  ut := U8(Token);
  Check(Fpdf_activate_license(PAnsiChar(ut)));
end;

class function Pdf.ExtractText(const PdfBytes: TBytes): string;
var
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  Check(Fpdf_extract_text(BytePtr(PdfBytes), Length(PdfBytes), P, Len));
  Result := Utf8BytesToString(TakeBuffer(P, Len));
end;

class function Pdf.Sign(const PdfBytes, KeyDer, CertDer: TBytes;
  const Reason, Location, Name: string; Pades: Boolean): TBytes;
var
  ur, ul, un: UTF8String;
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  ur := U8(Reason); ul := U8(Location); un := U8(Name);
  Check(Fpdf_sign(BytePtr(PdfBytes), Length(PdfBytes), BytePtr(KeyDer), Length(KeyDer),
        BytePtr(CertDer), Length(CertDer), OptU8(ur, Reason), OptU8(ul, Location),
        OptU8(un, Name), Ord(Pades), P, Len));
  Result := TakeBuffer(P, Len);
end;

class function Pdf.Timestamp(const PdfBytes, TsaKeyDer, TsaCertDer: TBytes;
  const Date: string): TBytes;
var
  ud: UTF8String;
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  ud := U8(Date);
  Check(Fpdf_timestamp(BytePtr(PdfBytes), Length(PdfBytes), BytePtr(TsaKeyDer), Length(TsaKeyDer),
        BytePtr(TsaCertDer), Length(TsaCertDer), OptU8(ud, Date), P, Len));
  Result := TakeBuffer(P, Len);
end;

class function Pdf.AddDss(const PdfBytes: TBytes; const Certs: array of TBytes;
  const Crls: array of TBytes): TBytes;
var
  certPtrs, crlPtrs: array of PByte;
  certLens, crlLens: array of NativeUInt;
  I: Integer;
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  SetLength(certPtrs, Length(Certs));
  SetLength(certLens, Length(Certs));
  for I := 0 to High(Certs) do
  begin
    certPtrs[I] := BytePtr(Certs[I]);
    certLens[I] := Length(Certs[I]);
  end;
  SetLength(crlPtrs, Length(Crls));
  SetLength(crlLens, Length(Crls));
  for I := 0 to High(Crls) do
  begin
    crlPtrs[I] := BytePtr(Crls[I]);
    crlLens[I] := Length(Crls[I]);
  end;
  Check(Fpdf_add_dss(BytePtr(PdfBytes), Length(PdfBytes),
        BPtr(certPtrs), SzPtr(certLens), Length(Certs),
        BPtr(crlPtrs), SzPtr(crlLens), Length(Crls), P, Len));
  Result := TakeBuffer(P, Len);
end;

end.
