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
    psLicense         = 12,
    psUnsupported     = 13
  );

  { PDF/A conformance level (argument to TPdfDocument.Pdfa). }
  { palA4/palA4E/palA4F are PDF/A-4 (ISO 19005-4), based on PDF 2.0. }
  TPdfaLevel = (palA1B, palA2B, palA2A, palA3B, palA3A, palA4, palA4E, palA4F);

  { Paragraph alignment. }
  TPdfAlign = (paLeft, paRight, paCenter, paJustify);

  { What Y means for TPdfEditable.PlaceText / PlaceParagraph. vaBaseline is the
    historical PlaceText behavior (Y = baseline); vaTop hangs the text from Y
    (baseline at Y - ascent x size, legacy fixed-position layout semantics); vaBottom
    rests the descender line on Y. vaLineTop/vaLineBottom anchor via the legacy layout engines
    "line box" (ascent+descent) -- for PlaceParagraph they also use the line-box
    height as the leading basis, so a single line and a wrapped block agree
    vertically. }
  TVerticalAnchor = (vaBaseline, vaTop, vaBottom, vaLineTop, vaLineBottom);

  { Vertical alignment of the line inside a TPdfEditable.MaskedText box:
    vlMiddle is the historical cap-height centering; vlTop hangs the line from
    the top edge (top line-alignment in rectangle-based text APIs semantics); vlBottom rests the
    descender line on the bottom edge. }
  TVerticalAlign = (vlTop, vlMiddle, vlBottom);

  { Coordinate space of the positioned stamping primitives (FillRect,
    PlaceText, MaskedText, PlaceParagraph, DrawImage). ssVisible (default):
    coordinates in the page's displayed space, compensating /Rotate. ssMedia:
    raw PDF user space (legacy fixed-position layout semantics) -- no composition
    with the page's /Rotate or crop offset. }
  TStampSpace = (ssVisible, ssMedia);

  { How a rotated TPdfEditable.DrawImage is anchored: iaCorner (default)
    rotates the image about its own lower-left corner at (X, Y); iaBoundingBox
    lands the rotated image's bounding box with its lower-left at (X, Y)
    (bounding-box layout semantics). }
  TImageAnchor = (iaCorner, iaBoundingBox);

  { Embedded-file relationship (PDF/A-3 attachments). }
  TAFRelationship = (afSource, afData, afAlternative, afSupplement, afUnspecified);

  { Encryption cipher (TPdfEditable.Encrypt). }
  TEncryption = (encRC4_128, encAES128, encAES256);

  { ZUGFeRD / Factur-X conformance profile (TPdfDocument.Facturx). }
  TFacturxProfile = (fxMinimum, fxBasicWL, fxBasic, fxEN16931, fxExtended);

  { DocMDP certification level applied by the first (certifying) signature:
    CertifyNone = not certifying; CertifyLocked = /P 1 (no changes); CertifyForms
    = /P 2 (form-filling + signing); CertifyFormsAndAnnotations = /P 3. }
  TCertify = (CertifyNone, CertifyLocked, CertifyForms, CertifyFormsAndAnnotations);

  { A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB). OID is the
    dotted-decimal policy OID; Hash is the policy-document digest (under
    HashAlgorithmOID, '' = SHA-256); URI ('' = none) is the SPURI qualifier. }
  TSignaturePolicy = record
    OID: UTF8String;
    Hash: TBytes;
    HashAlgorithmOID: UTF8String;
    URI: UTF8String;
  end;

  { Options for deferred / external signing (issue #41). Reason/Location/Name
    populate the signature dict; Pades selects ETSI.CAdES.detached; Certify
    applies DocMDP (only on the first signature); ContainerSize overrides the
    reserved /Contents size (0 = default 8192); set HasPolicy to attach Policy.
    Set Visible to draw a visible signature on page VisiblePage inside
    VisibleRect, with VisibleText (lines separated by #10) and/or an embedded
    VisibleImage (PNG/JPEG bytes). }
  TSigningOptions = record
    Reason: UTF8String;
    Location: UTF8String;
    Name: UTF8String;
    Pades: Boolean;
    Certify: TCertify;
    ContainerSize: Integer;
    HasPolicy: Boolean;
    Policy: TSignaturePolicy;
    Visible: Boolean;
    VisiblePage: NativeUInt;
    VisibleRect: array[0..3] of Double;  // [x0, y0, x1, y1] in points
    VisibleText: UTF8String;
    VisibleImage: TBytes;
  end;

  { A signature field discovered in a PDF (pre-signing inventory, Pdf.ListSignatures). }
  TSignatureField = record
    Name: UTF8String;
    Signed: Boolean;
  end;
  TSignatureFields = array of TSignatureField;

  { Model A "bring your own signer" callback: given the bytes to sign, return
    the raw RSA PKCS#1 v1.5 signature over their SHA-256 (typically by calling a
    remote HSM). The private key never reaches this library. A method pointer so
    it can carry state (FPC 3.2 has no anonymous methods). }
  TPdfRemoteSign = function(const DataToSign: TBytes): TBytes of object;

  { Model B two-phase signature in progress. Document is the prepared PDF (with a
    zero-filled /Contents placeholder); Bytes are the exact bytes the signature
    covers (the two ByteRange segments). Hand Hash to a remote signer, build a
    DER CMS/PKCS#7 container, then call Complete. Free with .Free. }
  TSigningSession = class(TObject)
  private
    FDocument: TBytes;
    FBytes: TBytes;
  public
    constructor Create(const ADocument, ABytes: TBytes);
    { SHA-256 of Bytes — the value a remote signer / HSM signs. }
    function Hash: TBytes;
    { Phase 2: embed a finished DER CMS/PKCS#7 container, returning the final
      signed PDF. }
    function Complete(const Container: TBytes): TBytes;
    property Document: TBytes read FDocument;
    property Bytes: TBytes read FBytes;
  end;

  { Dynamic array of strings (TPdfEditable.FieldNames). Declared locally so the
    unit stays self-contained across Delphi/FPC RTL versions. }
  TPdfStringArray = array of string;

  { Axis-aligned rectangle [x0,y0,x1,y1] for AcroForm widgets and redaction. }
  TPdfRect = record
    X0, Y0, X1, Y1: Double;
    { Non-negative extent of the rectangle. }
    function Width: Double;
    function Height: Double;
  end;

  { Read-only geometry of one page (Pdf.MeasurePage / MeasurePages). Width/Height
    are the unrotated size in PDF points; RotatedWidth/RotatedHeight account for
    /Rotate (swapped for 90/270 pages). MediaBox/CropBox are in PDF points. }
  TPageGeometry = record
    Page: Integer;
    Width, Height: Double;
    Rotation: Integer;
    RotatedWidth, RotatedHeight: Double;
    MediaBox, CropBox: TPdfRect;
  end;

  { A non-mutating summary of a PDF (Pdf.Inspect). PdfaLevel is '' when the file
    makes no PDF/A conformance claim; Encryption is '' (or 'None') for a plain
    file. RequiresPassword is True when the file is encrypted and the supplied
    (here, empty) password does not open it. }
  TPdfOverview = record
    Version: string;
    PdfaLevel: string;
    Encrypted: Boolean;
    Encryption: string;
    RequiresPassword: Boolean;
    PageCount: Integer;
  end;

  { One positional text match from Pdf.FindText. Page is 0-based; X/Y are the
    box origin (lower-left) and Width/Height its size, all in PDF points. }
  TTextHit = record
    Page: Integer;
    Text: string;
    X, Y, Width, Height: Double;
  end;
  TTextHits = array of TTextHit;

  { The two byte buffers produced by Pdf.BeginTimestamp (network-TSA / AD-RT):
    Document is the prepared PDF with a zero-filled /Contents placeholder; Tbs
    are the exact bytes the timestamp covers (hash these for the TSA request). }
  TTimestampSession = record
    Document: TBytes;
    Tbs: TBytes;
  end;

  { One radio button: its widget rectangle plus its /AP export value. }
  TRadioButton = record
    Rect: TPdfRect;
    ExportValue: string;
  end;
  TRadioButtons = array of TRadioButton;

  { A document outline (bookmark) entry. Build a tree with Child, then pass the
    root to TPdfDocument.AddBookmark, which pre-order flattens it. The object
    owns its children: freeing the root frees the whole tree. }
  TPdfBookmark = class(TObject)
  private
    FTitle: string;
    FPage: NativeUInt;
    FTop: Double;
    FHasTop: Boolean;
    FChildren: array of TPdfBookmark;
  public
    constructor Create(const ATitle: string; APage: NativeUInt); overload;
    constructor Create(const ATitle: string; APage: NativeUInt; ATop: Double); overload;
    destructor Destroy; override;
    { Append a child and return Self, so calls can be chained. }
    function Child(BM: TPdfBookmark): TPdfBookmark;
    property Title: string read FTitle;
    property Page: NativeUInt read FPage;
    property Top: Double read FTop;
    property HasTop: Boolean read FHasTop;
  end;

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
    function SetVersion(V: Integer): TPdfDocument;         // 0=1.4, 1=1.5, 2=1.7, 3=2.0
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

    { ---- hyperlinks, bookmarks, Factur-X ---- }
    function LinkUri(const R: TPdfRect; const Uri: string): TPdfDocument;
    function LinkToPage(const R: TPdfRect; PageIndex: NativeUInt): TPdfDocument; overload;
    function LinkToPage(const R: TPdfRect; PageIndex: NativeUInt; Top: Double): TPdfDocument; overload;
    function AddBookmark(BM: TPdfBookmark): TPdfDocument;
    function Facturx(const Xml: TBytes; Profile: TFacturxProfile = fxEN16931): TPdfDocument;

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
    function SetCheckbox(const Name: string; Checked: Boolean = True): Boolean;
    function SetRadio(const Name, ExportValue: string): Boolean;
    function SetChoice(const Name, Value: string): Boolean;
    function FlattenForms: TPdfEditable;
    function FieldNames: TPdfStringArray;
    function WatermarkText(const Text: string; Size: Double = 64.0;
                           R: Double = 0.5; G: Double = 0.5; B: Double = 0.5;
                           Opacity: Double = 0.30; RotationDeg: Double = 45.0;
                           OpaqueBackground: Boolean = False): TPdfEditable;
    function WatermarkImageFile(const Path: string; Width, Height: Double;
                                Opacity: Double = 0.30;
                                RotationDeg: Double = 0.0): TPdfEditable;
    function Redact(PageIndex: NativeUInt; const Rects: array of TPdfRect): Boolean;
    { Paint a filled rectangle (RGB, each 0..1) at Opacity (0..1) on page
      PageIndex (0-based). Coordinates are in the page's VISIBLE space (origin
      lower-left, y up), regardless of /Rotate. Returns False if the page does
      not exist. Issue #45 P1. }
    function FillRect(PageIndex: Integer; X, Y, Width, Height, R, G, B, Opacity: Double): Boolean;
    { Register a TrueType/OpenType stamping font (from a file path). The id is
      accepted by the FontId parameter of PlaceText / MaskedText /
      PlaceParagraph; the font is embedded as a subset, so stamped text renders
      with the real font's glyphs and metrics. }
    function AddFontFile(const Path: string): Integer;
    { Register a stamping font from raw TrueType/OpenType bytes. See AddFontFile. }
    function AddFont(const Data: TBytes): Integer;
    { Choose the coordinate space of the positioned stamping primitives
      (FillRect, PlaceText, MaskedText, PlaceParagraph, DrawImage) for
      subsequent calls: ssVisible (default, the page's displayed space,
      compensating /Rotate) or ssMedia (raw PDF user space, legacy layout semantics).
      Watermarks and redaction are unaffected. }
    procedure SetStampSpace(Space: TStampSpace);
    { Draw a line of text with its baseline at (X, Y) on page PageIndex
      (0-based), RGB (each 0..1). Coordinates are in the page's VISIBLE space
      (origin lower-left, y up), regardless of /Rotate (see SetStampSpace);
      RotationDeg rotates the text counter-clockwise about the (X, Y) anchor.
      FontId (from AddFontFile/AddFont) stamps with an embedded font; -1 uses
      the built-in Helvetica. Anchor says what Y means (vaBaseline = the
      historical baseline). Returns False if the page does not exist.
      Issue #45 P1. }
    function PlaceText(PageIndex: Integer; X, Y: Double; const Text: UTF8String;
                       Size, R, G, B, RotationDeg: Double;
                       Align: TPdfAlign = paLeft; FontId: Integer = -1;
                       Anchor: TVerticalAnchor = vaBaseline): Boolean;
    { Stamp a paragraph with automatic word wrapping: Text is broken into lines
      that fit Width points (#10 forces a break) and drawn downward from
      (X, Y). Anchor says what Y means for the block: vaTop (default) is the
      top of the box (first baseline at Y - ascent x Size, legacy layout engines
      fixed-position layout); vaBaseline is the first line's baseline;
      vaBottom/vaLineBottom pin the block's BOTTOM to Y (it grows upward by its
      real content height). MaxHeight > 0 is a height ceiling that truncates
      overflowing lines (0 = unlimited); LineHeight scales the default
      1.2 x Size leading. FontId as in PlaceText (the embedded font's metrics
      drive the break points). RotationDeg rotates the laid-out block
      counter-clockwise about the (X, Y) anchor. Returns False if the page (or
      font) was missing or the box was invalid. }
    function PlaceParagraph(PageIndex: Integer; X, Y, Width: Double;
                            const Text: UTF8String; Size: Double = 12.0;
                            R: Double = 0.0; G: Double = 0.0; B: Double = 0.0;
                            Align: TPdfAlign = paLeft; FontId: Integer = -1;
                            MaxHeight: Double = 0.0; LineHeight: Double = 1.0;
                            Anchor: TVerticalAnchor = vaTop;
                            RotationDeg: Double = 0.0): Boolean;
    { Like PlaceParagraph but also reports the number of lines drawn and the
      consumed block height in points (top of the first drawn line's box to the
      bottom of the last one's; 0 when nothing fit) -- stack blocks without
      re-measuring. }
    function PlaceParagraphMeasured(PageIndex: Integer; X, Y, Width: Double;
                                    const Text: UTF8String; Size, R, G, B: Double;
                                    Align: TPdfAlign; FontId: Integer;
                                    MaxHeight, LineHeight: Double;
                                    Anchor: TVerticalAnchor; RotationDeg: Double;
                                    out Lines: Integer; out Height: Double): Boolean;
    { Draw a line of standard-Helvetica text over an opaque background box
      [X, Y, X+Width, Y+Height] on page PageIndex (0-based): fills the box in the
      bg colour (each 0..1), then writes Text (Size points, text colour each 0..1)
      horizontally aligned per Align and vertically per VAlign (vlMiddle =
      historical cap-height centering) in the box. Coordinates are in the page's
      VISIBLE space (origin lower-left, y up), regardless of /Rotate. Text colour
      defaults to black, background to white. FontId (from AddFontFile/AddFont)
      stamps with an embedded font; -1 uses the built-in Helvetica. Padding is
      the horizontal edge inset (points) for paLeft/paRight (< 0 keeps the
      historical min(0.15 x Size, Width / 4); 0 starts flush with the box edge).
      Returns False if the page does not exist. Issue #50. }
    function MaskedText(PageIndex: Integer; X, Y, Width, Height: Double;
                        const Text: UTF8String; Size: Double;
                        TextR: Double = 0.0; TextG: Double = 0.0; TextB: Double = 0.0;
                        BgR: Double = 1.0; BgG: Double = 1.0; BgB: Double = 1.0;
                        Align: TPdfAlign = paLeft; FontId: Integer = -1;
                        VAlign: TVerticalAlign = vlMiddle;
                        Padding: Double = -1.0): Boolean;
    { Draw an image (PNG or JPEG bytes; the core dispatches on the signature) on
      page PageIndex (0-based), the image's lower-left corner at (X, Y), scaled to
      Width x Height points. Coordinates are in the page's VISIBLE space (origin
      lower-left, y up), regardless of /Rotate; RotationDeg rotates the image
      counter-clockwise about the anchor: iaCorner (default) is the image's own
      lower-left corner at (X, Y); iaBoundingBox lands the rotated image's
      bounding box with its lower-left at (X, Y) (bounding-box layout semantics).
      Returns False if the page does not exist. Issue #50. }
    function DrawImage(PageIndex: Integer; const Image: TBytes;
                       X, Y, Width, Height, RotationDeg: Double;
                       Anchor: TImageAnchor = iaCorner): Boolean;
    function ConvertToPdfa(Level: TPdfaLevel = palA2B): TPdfEditable;
    { Set the PDF version header (0=1.4, 1=1.5, 2=1.7, 3=2.0). }
    function SetVersion(V: Integer): TPdfEditable;
    { Strip PDF/A conformance (OutputIntents, pdfaid XMP), leaving a plain PDF. }
    function StripPdfa: TPdfEditable;
    { Normalize to a plain PDF at version V (strip PDF/A + set version). }
    function Normalize(V: Integer): TPdfEditable;
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
    { Extract the text of a single page (PageIndex, 0-based). Raises ERustPdf
      (psInvalidArgument) if the page is out of range. }
    class function ExtractPageText(const PdfBytes: TBytes; PageIndex: NativeUInt): string; static;
    class function ExtractImagesToDir(const PdfBytes: TBytes; const Dir: string): NativeUInt; static;
    { Render page PageIndex (0-based) of PdfBytes to a PNG at Dpi dots-per-inch.
      Page rendering is a licensed Pro feature: raises ERustPdf unless a license
      granting it is active. }
    class function RenderPageToPng(const PdfBytes: TBytes; PageIndex: NativeUInt; Dpi: Double): TBytes; static;
    { Number of pages in PdfBytes (free — no license required). }
    class function PageCount(const PdfBytes: TBytes): NativeUInt; static;
    { Validate every signature in PdfBytes and return the raw JSON array string
      (one object per signature with fields field_name, sub_filter, signer,
      covers_whole_document, digest_valid, signature_valid, is_valid, byte_range,
      plus the certificate detail fields issuer, serial_number, valid_from,
      valid_to, algorithm, signing_time (any may be null), cert_count and
      has_timestamp). Object Pascal has no bundled JSON parser, so this returns
      the JSON text verbatim; parse it with your preferred JSON unit. "[]" means
      unsigned. }
    class function VerifySignaturesJson(const PdfBytes: TBytes): UTF8String; static;

    { Positional full-text search. Returns one TTextHit per match (with its
      bounding box in PDF points, origin lower-left). CaseSensitive defaults to
      a case-insensitive search. }
    class function FindText(const PdfBytes: TBytes; const Query: string;
                            CaseSensitive: Boolean = False): TTextHits; static;

    { Per-page geometry (size, /Rotate, MediaBox, CropBox) of every page, in
      page order, without mutating the file. Issue #45 P1. }
    class function MeasurePages(const Pdf: TBytes): TArray<TPageGeometry>; static;
    { Geometry of a single page (0-based). Raises ERustPdf if Index is invalid. }
    class function MeasurePage(const Pdf: TBytes; Index: Integer): TPageGeometry; static;
    { Non-mutating summary: PDF version, PDF/A level, encryption posture and page
      count. Works even on a password-locked file. Issue #45 P1. }
    class function Inspect(const Pdf: TBytes): TPdfOverview; static;
    class function Sign(const PdfBytes, KeyDer, CertDer: TBytes;
                        const Reason: string = ''; const Location: string = '';
                        const Name: string = ''; Pades: Boolean = False): TBytes; static;
    class function Timestamp(const PdfBytes, TsaKeyDer, TsaCertDer: TBytes;
                             const Date: string = ''): TBytes; static;
    class function AddDss(const PdfBytes: TBytes;
                         const Certs: array of TBytes;
                         const Crls: array of TBytes): TBytes; static;

    { ---- deferred / external (HSM) signing — issue #41 ---- }

    { List the signature fields in PdfBytes (detect existing signatures before
      signing). An empty result means there are no signature fields. }
    class function ListSignatures(const PdfBytes: TBytes): TSignatureFields; static;

    { Model B, phase 1: prepare PdfBytes for deferred signing. Hand the returned
      session's Hash to a remote signer, build the DER CMS container, then call
      session.Complete. The caller owns the session (free with .Free). }
    class function BeginSigning(const PdfBytes: TBytes): TSigningSession; overload; static;
    class function BeginSigning(const PdfBytes: TBytes;
                                const Options: TSigningOptions): TSigningSession; overload; static;

    { Model B, phase 2: embed a complete DER CMS/PKCS#7 Container into a prepared
      Document (from BeginSigning), producing the final signed PDF. }
    class function CompleteSignature(const Document, Container: TBytes): TBytes; static;

    { Model A: sign PdfBytes without handing this library a key. It builds the
      CMS signed attributes and calls Callback for the raw RSA signature, then
      assembles and embeds the CMS. CertDer is the signer certificate; Chain are
      intermediate certificates (DER), supplied independently of the key. }
    class function SignWith(const PdfBytes, CertDer: TBytes;
                            Callback: TPdfRemoteSign): TBytes; overload; static;
    class function SignWith(const PdfBytes, CertDer: TBytes; Callback: TPdfRemoteSign;
                            const Chain: array of TBytes): TBytes; overload; static;
    class function SignWith(const PdfBytes, CertDer: TBytes; Callback: TPdfRemoteSign;
                            const Chain: array of TBytes;
                            const Options: TSigningOptions): TBytes; overload; static;

    { ---- network TSA / document timestamp (PAdES AD-RT) — issue #41 ---- }

    { Phase 1: prepare PdfBytes for a /DocTimeStamp. Hash the returned Tbs
      bytes, build a TSA request with TimestampRequest, POST it to the TSA,
      extract the token with TimestampTokenFromResponse, then embed it via
      CompleteSignature(session.Document, token). }
    class function BeginTimestamp(const PdfBytes: TBytes): TTimestampSession; static;

    { Build an RFC 3161 TimeStampReq for Imprint (a SHA-256 digest). Nonce is
      optional; CertReq asks the TSA to include its certificate. }
    class function TimestampRequest(const Imprint: TBytes;
                                    CertReq: Boolean = True): TBytes; overload; static;
    class function TimestampRequest(const Imprint, Nonce: TBytes;
                                    CertReq: Boolean = True): TBytes; overload; static;

    { Extract the TimeStampToken (CMS) from a TSA's RFC 3161 response. }
    class function TimestampTokenFromResponse(const Response: TBytes): TBytes; static;
  end;

{ Convenience constructor for a TPdfRect. }
function PdfRect(X0, Y0, X1, Y1: Double): TPdfRect;

{ A zero-initialised TSigningOptions (records have no constructors). }
function SigningOptions: TSigningOptions;

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
  Tpdf_page_link_uri        = function(doc: Pointer; x0, y0, x1, y1: Double; uri: PAnsiChar): Integer; cdecl;
  Tpdf_page_link_to_page    = function(doc: Pointer; x0, y0, x1, y1: Double; target_page: NativeUInt; top: Double; has_top: Integer): Integer; cdecl;
  Tpdf_document_add_bookmarks = function(doc: Pointer; count: NativeUInt; levels: Pointer; titles: Pointer; pages: Pointer; tops: Pointer; has_tops: Pointer): Integer; cdecl;
  Tpdf_document_facturx     = function(doc: Pointer; xml: PByte; len: NativeUInt; profile: Integer): Integer; cdecl;

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
  Tpdf_editable_set_checkbox = function(ed: Pointer; name: PAnsiChar; checked: Integer; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_set_radio   = function(ed: Pointer; name, export_value: PAnsiChar; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_set_choice  = function(ed: Pointer; name, value: PAnsiChar; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_flatten_forms = function(ed: Pointer): Integer; cdecl;
  Tpdf_editable_field_names = function(ed: Pointer; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_editable_watermark_text = function(ed: Pointer; text: PAnsiChar; size, r, g, b, opacity, rotation_deg: Double; opaque_background: Integer): Integer; cdecl;
  Tpdf_editable_watermark_image_file = function(ed: Pointer; path: PAnsiChar; width, height, opacity, rotation_deg: Double): Integer; cdecl;
  Tpdf_editable_redact      = function(ed: Pointer; index: NativeUInt; rects: Pointer; count: NativeUInt; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_convert_to_pdfa = function(ed: Pointer; level: Integer): Integer; cdecl;
  Tpdf_editable_set_version = function(ed: Pointer; version: Integer): Integer; cdecl;
  Tpdf_editable_strip_pdfa  = function(ed: Pointer): Integer; cdecl;
  Tpdf_editable_normalize   = function(ed: Pointer; version: Integer): Integer; cdecl;

  Tpdf_extract_text         = function(data: PByte; len: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_extract_images_to_dir = function(data: PByte; len: NativeUInt; dir: PAnsiChar; out out_count: NativeUInt): Integer; cdecl;
  Tpdf_render_page_to_png   = function(data: PByte; len: NativeUInt; page_index: NativeUInt; dpi: Double; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_page_count           = function(data: PByte; len: NativeUInt; out out_count: NativeUInt): Integer; cdecl;
  Tpdf_sign                 = function(pdf: PByte; pdf_len: NativeUInt; key: PByte; key_len: NativeUInt; cert: PByte; cert_len: NativeUInt; reason, location, name: PAnsiChar; pades: Integer; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_timestamp            = function(pdf: PByte; pdf_len: NativeUInt; key: PByte; key_len: NativeUInt; cert: PByte; cert_len: NativeUInt; date: PAnsiChar; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_add_dss              = function(pdf: PByte; pdf_len: NativeUInt; cert_ptrs: Pointer; cert_lens: Pointer; cert_count: NativeUInt; crl_ptrs: Pointer; crl_lens: Pointer; crl_count: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_verify_signatures_json = function(data: PByte; len: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_find_text_json       = function(data: PByte; len: NativeUInt; query: PAnsiChar; case_sensitive: Integer; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_measure_pages_json   = function(data: PByte; len: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_inspect_json         = function(data: PByte; len: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_editable_fill_rect   = function(ed: Pointer; index: Integer; x, y, width, height, r, g, b, opacity: Double; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_place_text  = function(ed: Pointer; index: Integer; x, y: Double; text: PAnsiChar; size, r, g, b, rotation_deg: Double; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_draw_image  = function(ed: Pointer; index: Integer; data: PByte; len: NativeUInt; x, y, width, height, rotation_deg: Double; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_place_text_aligned = function(ed: Pointer; index: Integer; x, y: Double; text: PAnsiChar; size, r, g, b, rotation_deg: Double; align: Integer; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_masked_text = function(ed: Pointer; index: Integer; x, y, width, height: Double; text: PAnsiChar; size, text_r, text_g, text_b, bg_r, bg_g, bg_b: Double; align: Integer; out out_found: Integer): Integer; cdecl;
  Tpdf_extract_page_text    = function(data: PByte; len: NativeUInt; page_index: NativeUInt; out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_editable_add_font_file = function(ed: Pointer; path: PAnsiChar; out outid: Integer): Integer; cdecl;
  Tpdf_editable_add_font    = function(ed: Pointer; data: PByte; len: NativeUInt; out outid: Integer): Integer; cdecl;
  Tpdf_editable_place_text_anchored = function(ed: Pointer; index: Integer; x, y: Double; text: PAnsiChar; size, r, g, b, rotation_deg: Double; align, anchor, font_id: Integer; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_masked_text_pad = function(ed: Pointer; index: Integer; x, y, width, height: Double; text: PAnsiChar; size, text_r, text_g, text_b, bg_r, bg_g, bg_b: Double; align, valign: Integer; pad: Double; font_id: Integer; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_place_paragraph_anchored = function(ed: Pointer; index: Integer; x, y, width: Double; text: PAnsiChar; size, r, g, b: Double; align, anchor, font_id: Integer; max_height, line_height, rotation_deg: Double; out out_height: Double; out out_lines: Integer; out out_found: Integer): Integer; cdecl;
  Tpdf_editable_set_stamp_space = function(ed: Pointer; space: Integer): Integer; cdecl;
  Tpdf_editable_draw_image_anchored = function(ed: Pointer; index: Integer; data: PByte; len: NativeUInt; x, y, width, height, rotation_deg: Double; anchor: Integer; out out_found: Integer): Integer; cdecl;

{ ---- deferred / external signing (issue #41) ---- }

type
  PSizeUInt = ^NativeUInt;

  { Mirrors the C-ABI PdfSigningOptions (include/pdf.h). NOT packed — laid out
    with natural C alignment (8-byte pointers/sizes, two contiguous 4-byte ints),
    which Delphi/FPC record alignment reproduces exactly on LP64 targets. NULL
    pointer fields and a zero EstimatedSize / PolicyHashLen mean "absent". }
  TPdfSigningOptionsC = record
    Reason: PAnsiChar;
    Location: PAnsiChar;
    Name: PAnsiChar;
    Pades: Integer;
    Certification: Integer;
    EstimatedSize: NativeUInt;
    PolicyOid: PAnsiChar;
    PolicyHash: PByte;
    PolicyHashLen: NativeUInt;
    PolicyHashAlgOid: PAnsiChar;
    PolicyUri: PAnsiChar;
    Visible: Integer;
    VisPage: NativeUInt;
    VisRect: array[0..3] of Double;
    VisText: PAnsiChar;
    VisImage: PByte;
    VisImageLen: NativeUInt;
  end;
  PPdfSigningOptionsC = ^TPdfSigningOptionsC;

  { Raw C callback: write the RSA signature of SHA-256(data) into sig_buf
    (capacity sig_cap), set sig_len^, return 0 on success (non-zero = failure). }
  TPdfSignHashFn = function(ctx: Pointer; data: PByte; data_len: NativeUInt;
    sig_buf: PByte; sig_cap: NativeUInt; sig_len: PSizeUInt): Integer; cdecl;

  Tpdf_sign_begin = function(pdf: PByte; pdf_len: NativeUInt; params: PPdfSigningOptionsC;
    out out_doc: PByte; out out_doc_len: NativeUInt;
    out out_tbs: PByte; out out_tbs_len: NativeUInt): Integer; cdecl;
  Tpdf_sign_complete = function(document: PByte; document_len: NativeUInt;
    container: PByte; container_len: NativeUInt;
    out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_sign_with = function(pdf: PByte; pdf_len: NativeUInt; cert_der: PByte; cert_len: NativeUInt;
    chain_ptrs: Pointer; chain_lens: Pointer; chain_count: NativeUInt;
    params: PPdfSigningOptionsC; callback: TPdfSignHashFn; ctx: Pointer;
    out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_list_signatures = function(pdf: PByte; pdf_len: NativeUInt;
    out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;

  Tpdf_timestamp_begin = function(pdf: PByte; pdf_len: NativeUInt;
    out out_doc: PByte; out out_doc_len: NativeUInt;
    out out_tbs: PByte; out out_tbs_len: NativeUInt): Integer; cdecl;
  Tpdf_timestamp_request = function(imprint: PByte; imprint_len: NativeUInt;
    nonce: PByte; nonce_len: NativeUInt; cert_req: Integer;
    out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;
  Tpdf_timestamp_token_from_response = function(response: PByte; response_len: NativeUInt;
    out outptr: PByte; out outlen: NativeUInt): Integer; cdecl;

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
  Fpdf_page_link_uri: Tpdf_page_link_uri;
  Fpdf_page_link_to_page: Tpdf_page_link_to_page;
  Fpdf_document_add_bookmarks: Tpdf_document_add_bookmarks;
  Fpdf_document_facturx: Tpdf_document_facturx;
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
  Fpdf_editable_set_checkbox: Tpdf_editable_set_checkbox;
  Fpdf_editable_set_radio: Tpdf_editable_set_radio;
  Fpdf_editable_set_choice: Tpdf_editable_set_choice;
  Fpdf_editable_flatten_forms: Tpdf_editable_flatten_forms;
  Fpdf_editable_field_names: Tpdf_editable_field_names;
  Fpdf_editable_watermark_text: Tpdf_editable_watermark_text;
  Fpdf_editable_watermark_image_file: Tpdf_editable_watermark_image_file;
  Fpdf_editable_redact: Tpdf_editable_redact;
  Fpdf_editable_convert_to_pdfa: Tpdf_editable_convert_to_pdfa;
  Fpdf_editable_set_version: Tpdf_editable_set_version;
  Fpdf_editable_strip_pdfa: Tpdf_editable_strip_pdfa;
  Fpdf_editable_normalize: Tpdf_editable_normalize;
  Fpdf_extract_text: Tpdf_extract_text;
  Fpdf_extract_images_to_dir: Tpdf_extract_images_to_dir;
  Fpdf_render_page_to_png: Tpdf_render_page_to_png;
  Fpdf_page_count: Tpdf_page_count;
  Fpdf_sign: Tpdf_sign;
  Fpdf_timestamp: Tpdf_timestamp;
  Fpdf_add_dss: Tpdf_add_dss;
  Fpdf_verify_signatures_json: Tpdf_verify_signatures_json;
  Fpdf_find_text_json: Tpdf_find_text_json;
  Fpdf_measure_pages_json: Tpdf_measure_pages_json;
  Fpdf_inspect_json: Tpdf_inspect_json;
  Fpdf_editable_fill_rect: Tpdf_editable_fill_rect;
  Fpdf_editable_place_text: Tpdf_editable_place_text;
  Fpdf_editable_draw_image: Tpdf_editable_draw_image;
  Fpdf_editable_place_text_aligned: Tpdf_editable_place_text_aligned;
  Fpdf_editable_masked_text: Tpdf_editable_masked_text;
  Fpdf_extract_page_text: Tpdf_extract_page_text;
  Fpdf_editable_add_font_file: Tpdf_editable_add_font_file;
  Fpdf_editable_add_font: Tpdf_editable_add_font;
  Fpdf_editable_place_text_anchored: Tpdf_editable_place_text_anchored;
  Fpdf_editable_masked_text_pad: Tpdf_editable_masked_text_pad;
  Fpdf_editable_place_paragraph_anchored: Tpdf_editable_place_paragraph_anchored;
  Fpdf_editable_set_stamp_space: Tpdf_editable_set_stamp_space;
  Fpdf_editable_draw_image_anchored: Tpdf_editable_draw_image_anchored;
  Fpdf_sign_begin: Tpdf_sign_begin;
  Fpdf_sign_complete: Tpdf_sign_complete;
  Fpdf_sign_with: Tpdf_sign_with;
  Fpdf_list_signatures: Tpdf_list_signatures;
  Fpdf_timestamp_begin: Tpdf_timestamp_begin;
  Fpdf_timestamp_request: Tpdf_timestamp_request;
  Fpdf_timestamp_token_from_response: Tpdf_timestamp_token_from_response;

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
  Fpdf_page_link_uri := Tpdf_page_link_uri(Bind('pdf_page_link_uri'));
  Fpdf_page_link_to_page := Tpdf_page_link_to_page(Bind('pdf_page_link_to_page'));
  Fpdf_document_add_bookmarks := Tpdf_document_add_bookmarks(Bind('pdf_document_add_bookmarks'));
  Fpdf_document_facturx := Tpdf_document_facturx(Bind('pdf_document_facturx'));
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
  Fpdf_editable_set_checkbox := Tpdf_editable_set_checkbox(Bind('pdf_editable_set_checkbox'));
  Fpdf_editable_set_radio := Tpdf_editable_set_radio(Bind('pdf_editable_set_radio'));
  Fpdf_editable_set_choice := Tpdf_editable_set_choice(Bind('pdf_editable_set_choice'));
  Fpdf_editable_flatten_forms := Tpdf_editable_flatten_forms(Bind('pdf_editable_flatten_forms'));
  Fpdf_editable_field_names := Tpdf_editable_field_names(Bind('pdf_editable_field_names'));
  Fpdf_editable_watermark_text := Tpdf_editable_watermark_text(Bind('pdf_editable_watermark_text'));
  Fpdf_editable_watermark_image_file := Tpdf_editable_watermark_image_file(Bind('pdf_editable_watermark_image_file'));
  Fpdf_editable_redact := Tpdf_editable_redact(Bind('pdf_editable_redact'));
  Fpdf_editable_convert_to_pdfa := Tpdf_editable_convert_to_pdfa(Bind('pdf_editable_convert_to_pdfa'));
  Fpdf_editable_set_version := Tpdf_editable_set_version(Bind('pdf_editable_set_version'));
  Fpdf_editable_strip_pdfa := Tpdf_editable_strip_pdfa(Bind('pdf_editable_strip_pdfa'));
  Fpdf_editable_normalize := Tpdf_editable_normalize(Bind('pdf_editable_normalize'));
  Fpdf_extract_text := Tpdf_extract_text(Bind('pdf_extract_text'));
  Fpdf_extract_images_to_dir := Tpdf_extract_images_to_dir(Bind('pdf_extract_images_to_dir'));
  Fpdf_render_page_to_png := Tpdf_render_page_to_png(Bind('pdf_render_page_to_png'));
  Fpdf_page_count := Tpdf_page_count(Bind('pdf_page_count'));
  Fpdf_sign := Tpdf_sign(Bind('pdf_sign'));
  Fpdf_timestamp := Tpdf_timestamp(Bind('pdf_timestamp'));
  Fpdf_add_dss := Tpdf_add_dss(Bind('pdf_add_dss'));
  Fpdf_verify_signatures_json := Tpdf_verify_signatures_json(Bind('pdf_verify_signatures_json'));
  Fpdf_find_text_json := Tpdf_find_text_json(Bind('pdf_find_text_json'));
  Fpdf_measure_pages_json := Tpdf_measure_pages_json(Bind('pdf_measure_pages_json'));
  Fpdf_inspect_json := Tpdf_inspect_json(Bind('pdf_inspect_json'));
  Fpdf_editable_fill_rect := Tpdf_editable_fill_rect(Bind('pdf_editable_fill_rect'));
  Fpdf_editable_place_text := Tpdf_editable_place_text(Bind('pdf_editable_place_text'));
  Fpdf_editable_draw_image := Tpdf_editable_draw_image(Bind('pdf_editable_draw_image'));
  Fpdf_editable_place_text_aligned := Tpdf_editable_place_text_aligned(Bind('pdf_editable_place_text_aligned'));
  Fpdf_editable_masked_text := Tpdf_editable_masked_text(Bind('pdf_editable_masked_text'));
  Fpdf_extract_page_text := Tpdf_extract_page_text(Bind('pdf_extract_page_text'));
  Fpdf_editable_add_font_file := Tpdf_editable_add_font_file(Bind('pdf_editable_add_font_file'));
  Fpdf_editable_add_font := Tpdf_editable_add_font(Bind('pdf_editable_add_font'));
  Fpdf_editable_place_text_anchored := Tpdf_editable_place_text_anchored(Bind('pdf_editable_place_text_anchored'));
  Fpdf_editable_masked_text_pad := Tpdf_editable_masked_text_pad(Bind('pdf_editable_masked_text_pad'));
  Fpdf_editable_place_paragraph_anchored := Tpdf_editable_place_paragraph_anchored(Bind('pdf_editable_place_paragraph_anchored'));
  Fpdf_editable_set_stamp_space := Tpdf_editable_set_stamp_space(Bind('pdf_editable_set_stamp_space'));
  Fpdf_editable_draw_image_anchored := Tpdf_editable_draw_image_anchored(Bind('pdf_editable_draw_image_anchored'));
  Fpdf_sign_begin := Tpdf_sign_begin(Bind('pdf_sign_begin'));
  Fpdf_sign_complete := Tpdf_sign_complete(Bind('pdf_sign_complete'));
  Fpdf_sign_with := Tpdf_sign_with(Bind('pdf_sign_with'));
  Fpdf_list_signatures := Tpdf_list_signatures(Bind('pdf_list_signatures'));
  Fpdf_timestamp_begin := Tpdf_timestamp_begin(Bind('pdf_timestamp_begin'));
  Fpdf_timestamp_request := Tpdf_timestamp_request(Bind('pdf_timestamp_request'));
  Fpdf_timestamp_token_from_response := Tpdf_timestamp_token_from_response(Bind('pdf_timestamp_token_from_response'));

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

{ ===================== SHA-256 (self-contained) ======================== }

{ A compact, dependency-free SHA-256 so TSigningSession.Hash works identically
  under Delphi and Free Pascal (this FPC ships no hash unit). Overflow/range
  checks are disabled locally because the compression function relies on the
  natural mod-2^32 wrap of Cardinal arithmetic. }

{$PUSH}
{$Q-}
{$R-}

function RotR32(X: Cardinal; N: Byte): Cardinal; inline;
begin
  Result := (X shr N) or (X shl (32 - N));
end;

function Sha256Bytes(const Data: TBytes): TBytes;
const
  K: array[0..63] of Cardinal = (
    $428a2f98, $71374491, $b5c0fbcf, $e9b5dba5, $3956c25b, $59f111f1, $923f82a4, $ab1c5ed5,
    $d807aa98, $12835b01, $243185be, $550c7dc3, $72be5d74, $80deb1fe, $9bdc06a7, $c19bf174,
    $e49b69c1, $efbe4786, $0fc19dc6, $240ca1cc, $2de92c6f, $4a7484aa, $5cb0a9dc, $76f988da,
    $983e5152, $a831c66d, $b00327c8, $bf597fc7, $c6e00bf3, $d5a79147, $06ca6351, $14292967,
    $27b70a85, $2e1b2138, $4d2c6dfc, $53380d13, $650a7354, $766a0abb, $81c2c92e, $92722c85,
    $a2bfe8a1, $a81a664b, $c24b8b70, $c76c51a3, $d192e819, $d6990624, $f40e3585, $106aa070,
    $19a4c116, $1e376c08, $2748774c, $34b0bcb5, $391c0cb3, $4ed8aa4a, $5b9cca4f, $682e6ff3,
    $748f82ee, $78a5636f, $84c87814, $8cc70208, $90befffa, $a4506ceb, $bef9a3f7, $c67178f2);
var
  H: array[0..7] of Cardinal;
  W: array[0..63] of Cardinal;
  a, b, c, d, e, f, g, hh, t1, t2, s0, s1, ch, maj: Cardinal;
  Msg: TBytes;
  ml: UInt64;
  i, t, base, nblocks, padLen: Integer;
begin
  H[0] := $6a09e667; H[1] := $bb67ae85; H[2] := $3c6ef372; H[3] := $a54ff53a;
  H[4] := $510e527f; H[5] := $9b05688c; H[6] := $1f83d9ab; H[7] := $5be0cd19;

  ml := UInt64(Length(Data)) * 8;
  padLen := Length(Data) + 1;
  while (padLen mod 64) <> 56 do
    Inc(padLen);
  SetLength(Msg, padLen + 8);
  if Length(Data) > 0 then
    Move(Data[0], Msg[0], Length(Data));
  Msg[Length(Data)] := $80;
  for i := Length(Data) + 1 to padLen - 1 do
    Msg[i] := 0;
  for i := 0 to 7 do
    Msg[padLen + i] := Byte((ml shr ((7 - i) * 8)) and $FF);

  nblocks := (padLen + 8) div 64;
  for base := 0 to nblocks - 1 do
  begin
    for t := 0 to 15 do
      W[t] := (Cardinal(Msg[base * 64 + t * 4]) shl 24) or
              (Cardinal(Msg[base * 64 + t * 4 + 1]) shl 16) or
              (Cardinal(Msg[base * 64 + t * 4 + 2]) shl 8) or
              (Cardinal(Msg[base * 64 + t * 4 + 3]));
    for t := 16 to 63 do
    begin
      s0 := RotR32(W[t - 15], 7) xor RotR32(W[t - 15], 18) xor (W[t - 15] shr 3);
      s1 := RotR32(W[t - 2], 17) xor RotR32(W[t - 2], 19) xor (W[t - 2] shr 10);
      W[t] := W[t - 16] + s0 + W[t - 7] + s1;
    end;
    a := H[0]; b := H[1]; c := H[2]; d := H[3];
    e := H[4]; f := H[5]; g := H[6]; hh := H[7];
    for t := 0 to 63 do
    begin
      s1 := RotR32(e, 6) xor RotR32(e, 11) xor RotR32(e, 25);
      ch := (e and f) xor ((not e) and g);
      t1 := hh + s1 + ch + K[t] + W[t];
      s0 := RotR32(a, 2) xor RotR32(a, 13) xor RotR32(a, 22);
      maj := (a and b) xor (a and c) xor (b and c);
      t2 := s0 + maj;
      hh := g; g := f; f := e; e := d + t1;
      d := c; c := b; b := a; a := t1 + t2;
    end;
    Inc(H[0], a); Inc(H[1], b); Inc(H[2], c); Inc(H[3], d);
    Inc(H[4], e); Inc(H[5], f); Inc(H[6], g); Inc(H[7], hh);
  end;

  SetLength(Result, 32);
  for i := 0 to 7 do
  begin
    Result[i * 4]     := Byte((H[i] shr 24) and $FF);
    Result[i * 4 + 1] := Byte((H[i] shr 16) and $FF);
    Result[i * 4 + 2] := Byte((H[i] shr 8) and $FF);
    Result[i * 4 + 3] := Byte(H[i] and $FF);
  end;
end;

{$POP}

{ ===================== signing-options helpers ========================= }

{ Pointer to a UTF-8 buffer, or nil for an empty string (NULL = "absent"). }
function U8Ptr(const U: UTF8String): PAnsiChar; inline;
begin
  if Length(U) = 0 then
    Result := nil
  else
    Result := PAnsiChar(U);
end;

{ Build the C options record. The PAnsiChar/PByte fields alias the caller's
  Options record, which stays alive for the duration of the native call. }
function MakeSignOpts(const Options: TSigningOptions): TPdfSigningOptionsC;
var
  I: Integer;
begin
  FillChar(Result, SizeOf(Result), 0);
  Result.Reason := U8Ptr(Options.Reason);
  Result.Location := U8Ptr(Options.Location);
  Result.Name := U8Ptr(Options.Name);
  Result.Pades := Ord(Options.Pades);
  Result.Certification := Ord(Options.Certify);
  if Options.ContainerSize > 0 then
    Result.EstimatedSize := NativeUInt(Options.ContainerSize);
  if Options.HasPolicy then
  begin
    Result.PolicyOid := U8Ptr(Options.Policy.OID);
    if Length(Options.Policy.Hash) > 0 then
    begin
      Result.PolicyHash := BytePtr(Options.Policy.Hash);
      Result.PolicyHashLen := Length(Options.Policy.Hash);
    end;
    Result.PolicyHashAlgOid := U8Ptr(Options.Policy.HashAlgorithmOID);
    Result.PolicyUri := U8Ptr(Options.Policy.URI);
  end;
  if Options.Visible then
  begin
    Result.Visible := 1;
    Result.VisPage := Options.VisiblePage;
    for I := 0 to 3 do
      Result.VisRect[I] := Options.VisibleRect[I];
    Result.VisText := U8Ptr(Options.VisibleText);
    if Length(Options.VisibleImage) > 0 then
    begin
      Result.VisImage := BytePtr(Options.VisibleImage);
      Result.VisImageLen := Length(Options.VisibleImage);
    end;
  end;
end;

{ The Model A signer is threaded through a per-thread variable rather than the
  C ctx pointer: the native call invokes the callback synchronously on the same
  thread, so this is reentrant across threads, and it sidesteps the Delphi/FPC
  divergence in what `@` of a method-pointer variable yields. }
threadvar
  GActiveSigner: TPdfRemoteSign;

{ The single global cdecl trampoline for Model A: invoke the active signer,
  copying its signature into the native-supplied buffer. }
function SignHashTrampoline(ctx: Pointer; data: PByte; data_len: NativeUInt;
  sig_buf: PByte; sig_cap: NativeUInt; sig_len: PSizeUInt): Integer; cdecl;
var
  inBytes, sig: TBytes;
  cb: TPdfRemoteSign;
begin
  try
    cb := GActiveSigner;
    if not Assigned(cb) then
      Exit(3);  { no signer registered for this thread }
    SetLength(inBytes, data_len);
    if data_len > 0 then
      Move(data^, inBytes[0], data_len);
    sig := cb(inBytes);
    if NativeUInt(Length(sig)) > sig_cap then
      Exit(2);  { signature larger than the reserved buffer }
    if Length(sig) > 0 then
      Move(sig[0], sig_buf^, Length(sig));
    sig_len^ := NativeUInt(Length(sig));
    Result := 0;
  except
    Result := 1;  { the signer raised }
  end;
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

function TPdfRect.Width: Double;
begin
  Result := Abs(X1 - X0);
end;

function TPdfRect.Height: Double;
begin
  Result := Abs(Y1 - Y0);
end;

function PdfRect(X0, Y0, X1, Y1: Double): TPdfRect;
begin
  Result.X0 := X0;
  Result.Y0 := Y0;
  Result.X1 := X1;
  Result.Y1 := Y1;
end;

function SigningOptions: TSigningOptions;
var
  I: Integer;
begin
  { A managed local record is already zero-initialised; return it as-is. }
  Result.Reason := '';
  Result.Location := '';
  Result.Name := '';
  Result.Pades := False;
  Result.Certify := CertifyNone;
  Result.ContainerSize := 0;
  Result.HasPolicy := False;
  Result.Visible := False;
  Result.VisiblePage := 0;
  for I := 0 to 3 do
    Result.VisibleRect[I] := 0.0;
  Result.VisibleText := '';
  Result.VisibleImage := nil;
end;

{ ===================== TSigningSession ================================= }

constructor TSigningSession.Create(const ADocument, ABytes: TBytes);
begin
  inherited Create;
  FDocument := ADocument;
  FBytes := ABytes;
end;

function TSigningSession.Hash: TBytes;
begin
  Result := Sha256Bytes(FBytes);
end;

function TSigningSession.Complete(const Container: TBytes): TBytes;
begin
  Result := Pdf.CompleteSignature(FDocument, Container);
end;

{ ===================== TPdfBookmark =================================== }

constructor TPdfBookmark.Create(const ATitle: string; APage: NativeUInt);
begin
  inherited Create;
  FTitle := ATitle;
  FPage := APage;
  FTop := 0.0;
  FHasTop := False;
end;

constructor TPdfBookmark.Create(const ATitle: string; APage: NativeUInt; ATop: Double);
begin
  inherited Create;
  FTitle := ATitle;
  FPage := APage;
  FTop := ATop;
  FHasTop := True;
end;

destructor TPdfBookmark.Destroy;
var
  I: Integer;
begin
  for I := 0 to High(FChildren) do
    FChildren[I].Free;
  FChildren := nil;
  inherited Destroy;
end;

function TPdfBookmark.Child(BM: TPdfBookmark): TPdfBookmark;
begin
  SetLength(FChildren, Length(FChildren) + 1);
  FChildren[High(FChildren)] := BM;
  Result := Self;
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

function TPdfDocument.LinkUri(const R: TPdfRect; const Uri: string): TPdfDocument;
var
  uu: UTF8String;
begin
  uu := U8(Uri);
  Check(Fpdf_page_link_uri(H, R.X0, R.Y0, R.X1, R.Y1, PAnsiChar(uu)));
  Result := Self;
end;

function TPdfDocument.LinkToPage(const R: TPdfRect; PageIndex: NativeUInt): TPdfDocument;
begin
  Check(Fpdf_page_link_to_page(H, R.X0, R.Y0, R.X1, R.Y1, PageIndex, 0.0, 0));
  Result := Self;
end;

function TPdfDocument.LinkToPage(const R: TPdfRect; PageIndex: NativeUInt; Top: Double): TPdfDocument;
begin
  Check(Fpdf_page_link_to_page(H, R.X0, R.Y0, R.X1, R.Y1, PageIndex, Top, 1));
  Result := Self;
end;

function TPdfDocument.AddBookmark(BM: TPdfBookmark): TPdfDocument;
var
  levels: array of Integer;
  pages: array of NativeUInt;
  tops: array of Double;
  hasTops: array of Integer;
  titlesU8: array of UTF8String;
  titlePtrs: array of PAnsiChar;
  N, I: Integer;

  { Pre-order flatten: append the node, then recurse into children, with the
    child level one deeper (root = level 0). }
  procedure Walk(Node: TPdfBookmark; Level: Integer);
  var
    J: Integer;
  begin
    SetLength(levels, N + 1);
    SetLength(pages, N + 1);
    SetLength(tops, N + 1);
    SetLength(hasTops, N + 1);
    SetLength(titlesU8, N + 1);
    levels[N] := Level;
    pages[N] := Node.FPage;
    titlesU8[N] := U8(Node.FTitle);
    if Node.FHasTop then
    begin
      tops[N] := Node.FTop;
      hasTops[N] := 1;
    end
    else
    begin
      tops[N] := 0.0;
      hasTops[N] := 0;
    end;
    Inc(N);
    for J := 0 to High(Node.FChildren) do
      Walk(Node.FChildren[J], Level + 1);
  end;

begin
  if BM = nil then
  begin
    Result := Self;
    Exit;
  end;
  N := 0;
  Walk(BM, 0);
  SetLength(titlePtrs, N);
  for I := 0 to N - 1 do
    titlePtrs[I] := PAnsiChar(titlesU8[I]);
  Check(Fpdf_document_add_bookmarks(H, N,
        @levels[0], StrPtr(titlePtrs), SzPtr(pages),
        DblPtr(tops), @hasTops[0]));
  Result := Self;
end;

function TPdfDocument.Facturx(const Xml: TBytes; Profile: TFacturxProfile): TPdfDocument;
begin
  Check(Fpdf_document_facturx(H, BytePtr(Xml), Length(Xml), Ord(Profile)));
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

function TPdfEditable.SetCheckbox(const Name: string; Checked: Boolean): Boolean;
var
  un: UTF8String;
  found: Integer;
begin
  un := U8(Name);
  found := 0;
  Check(Fpdf_editable_set_checkbox(H, PAnsiChar(un), Ord(Checked), found));
  Result := found <> 0;
end;

function TPdfEditable.SetRadio(const Name, ExportValue: string): Boolean;
var
  un, uv: UTF8String;
  found: Integer;
begin
  un := U8(Name); uv := U8(ExportValue);
  found := 0;
  Check(Fpdf_editable_set_radio(H, PAnsiChar(un), PAnsiChar(uv), found));
  Result := found <> 0;
end;

function TPdfEditable.SetChoice(const Name, Value: string): Boolean;
var
  un, uv: UTF8String;
  found: Integer;
begin
  un := U8(Name); uv := U8(Value);
  found := 0;
  Check(Fpdf_editable_set_choice(H, PAnsiChar(un), PAnsiChar(uv), found));
  Result := found <> 0;
end;

function TPdfEditable.FlattenForms: TPdfEditable;
begin
  Check(Fpdf_editable_flatten_forms(H));
  Result := Self;
end;

function TPdfEditable.FieldNames: TPdfStringArray;
var
  P: PByte;
  Len: NativeUInt;
  Joined: string;
  Lines: TStringList;
  I: Integer;
begin
  Check(Fpdf_editable_field_names(H, P, Len));
  Joined := Utf8BytesToString(TakeBuffer(P, Len));
  Lines := TStringList.Create;
  try
    { Split on newline; skip empty lines (trailing newline / blank names). }
    Lines.Text := Joined;
    SetLength(Result, 0);
    for I := 0 to Lines.Count - 1 do
      if Lines[I] <> '' then
      begin
        SetLength(Result, Length(Result) + 1);
        Result[High(Result)] := Lines[I];
      end;
  finally
    Lines.Free;
  end;
end;

function TPdfEditable.WatermarkText(const Text: string; Size, R, G, B,
  Opacity, RotationDeg: Double; OpaqueBackground: Boolean): TPdfEditable;
var
  ut: UTF8String;
begin
  ut := U8(Text);
  Check(Fpdf_editable_watermark_text(H, PAnsiChar(ut), Size, R, G, B, Opacity,
        RotationDeg, Ord(OpaqueBackground)));
  Result := Self;
end;

function TPdfEditable.WatermarkImageFile(const Path: string; Width, Height,
  Opacity, RotationDeg: Double): TPdfEditable;
var
  up: UTF8String;
begin
  up := U8(Path);
  Check(Fpdf_editable_watermark_image_file(H, PAnsiChar(up), Width, Height, Opacity, RotationDeg));
  Result := Self;
end;

function TPdfEditable.Redact(PageIndex: NativeUInt; const Rects: array of TPdfRect): Boolean;
var
  flat: array of Double;
  N, I: Integer;
  found: Integer;
begin
  N := Length(Rects);
  SetLength(flat, N * 4);
  for I := 0 to N - 1 do
  begin
    flat[I * 4 + 0] := Rects[I].X0;
    flat[I * 4 + 1] := Rects[I].Y0;
    flat[I * 4 + 2] := Rects[I].X1;
    flat[I * 4 + 3] := Rects[I].Y1;
  end;
  found := 0;
  Check(Fpdf_editable_redact(H, PageIndex, DblPtr(flat), N, found));
  Result := found <> 0;
end;

function TPdfEditable.FillRect(PageIndex: Integer; X, Y, Width, Height,
  R, G, B, Opacity: Double): Boolean;
var
  found: Integer;
begin
  found := 0;
  Check(Fpdf_editable_fill_rect(H, PageIndex, X, Y, Width, Height, R, G, B,
        Opacity, found));
  Result := found <> 0;
end;

function TPdfEditable.AddFontFile(const Path: string): Integer;
var
  up: UTF8String;
  id: Integer;
begin
  up := U8(Path);
  id := 0;
  Check(Fpdf_editable_add_font_file(H, PAnsiChar(up), id));
  Result := id;
end;

function TPdfEditable.AddFont(const Data: TBytes): Integer;
var
  id: Integer;
begin
  id := 0;
  Check(Fpdf_editable_add_font(H, BytePtr(Data), Length(Data), id));
  Result := id;
end;

procedure TPdfEditable.SetStampSpace(Space: TStampSpace);
begin
  Check(Fpdf_editable_set_stamp_space(H, Ord(Space)));
end;

function TPdfEditable.PlaceText(PageIndex: Integer; X, Y: Double;
  const Text: UTF8String; Size, R, G, B, RotationDeg: Double;
  Align: TPdfAlign; FontId: Integer; Anchor: TVerticalAnchor): Boolean;
var
  found: Integer;
begin
  found := 0;
  Check(Fpdf_editable_place_text_anchored(H, PageIndex, X, Y, PAnsiChar(Text), Size,
        R, G, B, RotationDeg, Ord(Align), Ord(Anchor), FontId, found));
  Result := found <> 0;
end;

function TPdfEditable.PlaceParagraph(PageIndex: Integer; X, Y, Width: Double;
  const Text: UTF8String; Size, R, G, B: Double; Align: TPdfAlign;
  FontId: Integer; MaxHeight, LineHeight: Double; Anchor: TVerticalAnchor;
  RotationDeg: Double): Boolean;
var
  height: Double;
  lines, found: Integer;
begin
  height := 0.0;
  lines := 0;
  found := 0;
  Check(Fpdf_editable_place_paragraph_anchored(H, PageIndex, X, Y, Width,
        PAnsiChar(Text), Size, R, G, B, Ord(Align), Ord(Anchor), FontId,
        MaxHeight, LineHeight, RotationDeg, height, lines, found));
  Result := found <> 0;
end;

function TPdfEditable.PlaceParagraphMeasured(PageIndex: Integer; X, Y, Width: Double;
  const Text: UTF8String; Size, R, G, B: Double; Align: TPdfAlign;
  FontId: Integer; MaxHeight, LineHeight: Double; Anchor: TVerticalAnchor;
  RotationDeg: Double; out Lines: Integer; out Height: Double): Boolean;
var
  found: Integer;
begin
  Lines := 0;
  Height := 0.0;
  found := 0;
  Check(Fpdf_editable_place_paragraph_anchored(H, PageIndex, X, Y, Width,
        PAnsiChar(Text), Size, R, G, B, Ord(Align), Ord(Anchor), FontId,
        MaxHeight, LineHeight, RotationDeg, Height, Lines, found));
  Result := found <> 0;
end;

function TPdfEditable.MaskedText(PageIndex: Integer; X, Y, Width, Height: Double;
  const Text: UTF8String; Size: Double; TextR, TextG, TextB, BgR, BgG, BgB: Double;
  Align: TPdfAlign; FontId: Integer; VAlign: TVerticalAlign;
  Padding: Double): Boolean;
var
  found: Integer;
begin
  found := 0;
  Check(Fpdf_editable_masked_text_pad(H, PageIndex, X, Y, Width, Height,
        PAnsiChar(Text), Size, TextR, TextG, TextB, BgR, BgG, BgB, Ord(Align),
        Ord(VAlign), Padding, FontId, found));
  Result := found <> 0;
end;

function TPdfEditable.DrawImage(PageIndex: Integer; const Image: TBytes;
  X, Y, Width, Height, RotationDeg: Double; Anchor: TImageAnchor): Boolean;
var
  found: Integer;
begin
  found := 0;
  Check(Fpdf_editable_draw_image_anchored(H, PageIndex, BytePtr(Image), Length(Image),
        X, Y, Width, Height, RotationDeg, Ord(Anchor), found));
  Result := found <> 0;
end;

function TPdfEditable.ConvertToPdfa(Level: TPdfaLevel): TPdfEditable;
begin
  Check(Fpdf_editable_convert_to_pdfa(H, Ord(Level)));
  Result := Self;
end;

function TPdfEditable.SetVersion(V: Integer): TPdfEditable;
begin
  Check(Fpdf_editable_set_version(H, V));
  Result := Self;
end;

function TPdfEditable.StripPdfa: TPdfEditable;
begin
  Check(Fpdf_editable_strip_pdfa(H));
  Result := Self;
end;

function TPdfEditable.Normalize(V: Integer): TPdfEditable;
begin
  Check(Fpdf_editable_normalize(H, V));
  Result := Self;
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

{ ===================== minimal JSON (find_text) ======================== }

{ A small, dependency-free JSON reader for the flat array Pdf.FindText returns:
  an array of objects, each with the keys page (int), text (string) and x, y,
  width, height (numbers). It walks the raw UTF-8 bytes (1-based index P over
  the UTF8String J) to avoid codepage surprises; string values are decoded
  UTF-8 -> string at the end. It is tolerant of whitespace and key order, and
  recognises only the keys above. Mirrors the binding's self-contained style
  (cf. the bundled SHA-256). }

procedure JsonSkipWs(const J: UTF8String; var P: Integer); inline;
begin
  while (P <= Length(J)) and (J[P] in [#9, #10, #13, ' ']) do
    Inc(P);
end;

function JsonParseString(const J: UTF8String; var P: Integer): string;
var
  Buf: UTF8String;
  C: AnsiChar;
  Code, Lo: Integer;

  function HexNibble(Ch: AnsiChar): Integer;
  begin
    case Ch of
      '0'..'9': Result := Ord(Ch) - Ord('0');
      'a'..'f': Result := Ord(Ch) - Ord('a') + 10;
      'A'..'F': Result := Ord(Ch) - Ord('A') + 10;
    else
      Result := 0;
    end;
  end;

  function Read4Hex: Integer;
  var
    K: Integer;
  begin
    Result := 0;
    for K := 1 to 4 do
      if P <= Length(J) then
      begin
        Result := (Result shl 4) or HexNibble(J[P]);
        Inc(P);
      end;
  end;

  procedure AppendCodepoint(CP: Integer);
  begin
    if CP < $80 then
      Buf := Buf + AnsiChar(CP)
    else if CP < $800 then
    begin
      Buf := Buf + AnsiChar($C0 or (CP shr 6));
      Buf := Buf + AnsiChar($80 or (CP and $3F));
    end
    else if CP < $10000 then
    begin
      Buf := Buf + AnsiChar($E0 or (CP shr 12));
      Buf := Buf + AnsiChar($80 or ((CP shr 6) and $3F));
      Buf := Buf + AnsiChar($80 or (CP and $3F));
    end
    else
    begin
      Buf := Buf + AnsiChar($F0 or (CP shr 18));
      Buf := Buf + AnsiChar($80 or ((CP shr 12) and $3F));
      Buf := Buf + AnsiChar($80 or ((CP shr 6) and $3F));
      Buf := Buf + AnsiChar($80 or (CP and $3F));
    end;
  end;

begin
  Buf := '';
  Inc(P);  { skip opening quote }
  while (P <= Length(J)) and (J[P] <> '"') do
  begin
    C := J[P];
    if C = '\' then
    begin
      Inc(P);
      if P > Length(J) then
        Break;
      C := J[P];
      Inc(P);  { consume the escape selector }
      case C of
        '"': Buf := Buf + '"';
        '\': Buf := Buf + '\';
        '/': Buf := Buf + '/';
        'b': Buf := Buf + #8;
        'f': Buf := Buf + #12;
        'n': Buf := Buf + #10;
        'r': Buf := Buf + #13;
        't': Buf := Buf + #9;
        'u':
          begin
            Code := Read4Hex;
            if (Code >= $D800) and (Code <= $DBFF) and (P + 1 <= Length(J)) and
               (J[P] = '\') and (J[P + 1] = 'u') then
            begin
              Inc(P, 2);  { skip the low surrogate's \u }
              Lo := Read4Hex;
              Code := $10000 + ((Code - $D800) shl 10) + (Lo - $DC00);
            end;
            AppendCodepoint(Code);
          end;
      else
        Buf := Buf + C;
      end;
    end
    else
    begin
      Buf := Buf + C;
      Inc(P);
    end;
  end;
  if (P <= Length(J)) and (J[P] = '"') then
    Inc(P);  { skip closing quote }
  Result := string(Buf);
end;

function JsonParseNumber(const J: UTF8String; var P: Integer): Double;
var
  StartP: Integer;
  FS: TFormatSettings;
begin
  StartP := P;
  while (P <= Length(J)) and (J[P] in ['0'..'9', '+', '-', '.', 'e', 'E']) do
    Inc(P);
  FS := FormatSettings;
  FS.DecimalSeparator := '.';
  FS.ThousandSeparator := #0;
  Result := StrToFloatDef(string(Copy(J, StartP, P - StartP)), 0.0, FS);
end;

function JsonParseHit(const J: UTF8String; var P: Integer): TTextHit;
var
  Key: string;
begin
  Result.Page := 0;
  Result.Text := '';
  Result.X := 0.0; Result.Y := 0.0; Result.Width := 0.0; Result.Height := 0.0;
  JsonSkipWs(J, P);
  if (P <= Length(J)) and (J[P] = '{') then
    Inc(P);
  while P <= Length(J) do
  begin
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = '}' then
    begin
      Inc(P);
      Break;
    end;
    if J[P] = ',' then
    begin
      Inc(P);
      Continue;
    end;
    if J[P] <> '"' then
      Break;  { malformed }
    Key := JsonParseString(J, P);
    JsonSkipWs(J, P);
    if (P <= Length(J)) and (J[P] = ':') then
      Inc(P);
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = '"' then
    begin
      if Key = 'text' then
        Result.Text := JsonParseString(J, P)
      else
        JsonParseString(J, P);  { discard unknown string }
    end
    else if J[P] = 'n' then
      Inc(P, 4)  { null }
    else
    begin
      if Key = 'page' then
        Result.Page := Round(JsonParseNumber(J, P))
      else if Key = 'x' then
        Result.X := JsonParseNumber(J, P)
      else if Key = 'y' then
        Result.Y := JsonParseNumber(J, P)
      else if Key = 'width' then
        Result.Width := JsonParseNumber(J, P)
      else if Key = 'height' then
        Result.Height := JsonParseNumber(J, P)
      else
        JsonParseNumber(J, P);  { discard unknown number }
    end;
  end;
end;

function ParseTextHits(const J: UTF8String): TTextHits;
var
  P, N: Integer;
begin
  SetLength(Result, 0);
  N := 0;
  P := 1;
  JsonSkipWs(J, P);
  if (P > Length(J)) or (J[P] <> '[') then
    Exit;
  Inc(P);
  while P <= Length(J) do
  begin
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = ']' then
      Break;
    if J[P] = ',' then
    begin
      Inc(P);
      Continue;
    end;
    if J[P] <> '{' then
      Break;
    SetLength(Result, N + 1);
    Result[N] := JsonParseHit(J, P);
    Inc(N);
  end;
end;

{ ===================== minimal JSON (measure + inspect) =============== }

{ Parse a 4-element JSON number array [x0,y0,x1,y1] into a TPdfRect; tolerant of
  whitespace and extra/missing elements (missing -> 0). Reuses the find_text
  number/whitespace scanners above. }
function JsonParseRectArray(const J: UTF8String; var P: Integer): TPdfRect;
var
  Vals: array[0..3] of Double;
  N: Integer;
begin
  Vals[0] := 0.0; Vals[1] := 0.0; Vals[2] := 0.0; Vals[3] := 0.0;
  JsonSkipWs(J, P);
  if (P <= Length(J)) and (J[P] = '[') then
    Inc(P);
  N := 0;
  while P <= Length(J) do
  begin
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = ']' then
    begin
      Inc(P);
      Break;
    end;
    if J[P] = ',' then
    begin
      Inc(P);
      Continue;
    end;
    if N <= 3 then
      Vals[N] := JsonParseNumber(J, P)
    else
      JsonParseNumber(J, P);  { discard surplus }
    Inc(N);
  end;
  Result.X0 := Vals[0]; Result.Y0 := Vals[1];
  Result.X1 := Vals[2]; Result.Y1 := Vals[3];
end;

function JsonParseGeometry(const J: UTF8String; var P: Integer): TPageGeometry;
var
  Key: string;
begin
  Result.Page := 0;
  Result.Width := 0.0; Result.Height := 0.0;
  Result.Rotation := 0;
  Result.RotatedWidth := 0.0; Result.RotatedHeight := 0.0;
  Result.MediaBox := PdfRect(0, 0, 0, 0);
  Result.CropBox := PdfRect(0, 0, 0, 0);
  JsonSkipWs(J, P);
  if (P <= Length(J)) and (J[P] = '{') then
    Inc(P);
  while P <= Length(J) do
  begin
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = '}' then
    begin
      Inc(P);
      Break;
    end;
    if J[P] = ',' then
    begin
      Inc(P);
      Continue;
    end;
    if J[P] <> '"' then
      Break;  { malformed }
    Key := JsonParseString(J, P);
    JsonSkipWs(J, P);
    if (P <= Length(J)) and (J[P] = ':') then
      Inc(P);
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = '[' then
    begin
      if Key = 'mediaBox' then
        Result.MediaBox := JsonParseRectArray(J, P)
      else if Key = 'cropBox' then
        Result.CropBox := JsonParseRectArray(J, P)
      else
        JsonParseRectArray(J, P);  { discard unknown array }
    end
    else if J[P] = '"' then
      JsonParseString(J, P)  { discard unexpected string }
    else if J[P] = 'n' then
      Inc(P, 4)  { null }
    else if J[P] = 't' then
      Inc(P, 4)  { true }
    else if J[P] = 'f' then
      Inc(P, 5)  { false }
    else
    begin
      if Key = 'page' then
        Result.Page := Round(JsonParseNumber(J, P))
      else if Key = 'rotation' then
        Result.Rotation := Round(JsonParseNumber(J, P))
      else if Key = 'width' then
        Result.Width := JsonParseNumber(J, P)
      else if Key = 'height' then
        Result.Height := JsonParseNumber(J, P)
      else if Key = 'rotatedWidth' then
        Result.RotatedWidth := JsonParseNumber(J, P)
      else if Key = 'rotatedHeight' then
        Result.RotatedHeight := JsonParseNumber(J, P)
      else
        JsonParseNumber(J, P);  { discard unknown number }
    end;
  end;
end;

function ParseGeometries(const J: UTF8String): TArray<TPageGeometry>;
var
  P, N: Integer;
begin
  SetLength(Result, 0);
  N := 0;
  P := 1;
  JsonSkipWs(J, P);
  if (P > Length(J)) or (J[P] <> '[') then
    Exit;
  Inc(P);
  while P <= Length(J) do
  begin
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = ']' then
      Break;
    if J[P] = ',' then
    begin
      Inc(P);
      Continue;
    end;
    if J[P] <> '{' then
      Break;
    SetLength(Result, N + 1);
    Result[N] := JsonParseGeometry(J, P);
    Inc(N);
  end;
end;

{ Parse the flat inspect object. Recognises version/pdfaLevel/encryption
  (strings, pdfaLevel may be null -> ''), encrypted/requiresPassword (booleans)
  and pageCount (number). }
function ParseOverview(const J: UTF8String): TPdfOverview;
var
  P: Integer;
  Key: string;
begin
  Result.Version := '';
  Result.PdfaLevel := '';
  Result.Encrypted := False;
  Result.Encryption := '';
  Result.RequiresPassword := False;
  Result.PageCount := 0;
  P := 1;
  JsonSkipWs(J, P);
  if (P <= Length(J)) and (J[P] = '{') then
    Inc(P);
  while P <= Length(J) do
  begin
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = '}' then
      Break;
    if J[P] = ',' then
    begin
      Inc(P);
      Continue;
    end;
    if J[P] <> '"' then
      Break;  { malformed }
    Key := JsonParseString(J, P);
    JsonSkipWs(J, P);
    if (P <= Length(J)) and (J[P] = ':') then
      Inc(P);
    JsonSkipWs(J, P);
    if P > Length(J) then
      Break;
    if J[P] = '"' then
    begin
      if Key = 'version' then
        Result.Version := JsonParseString(J, P)
      else if Key = 'pdfaLevel' then
        Result.PdfaLevel := JsonParseString(J, P)
      else if Key = 'encryption' then
        Result.Encryption := JsonParseString(J, P)
      else
        JsonParseString(J, P);  { discard unknown string }
    end
    else if J[P] = 't' then
    begin
      if Key = 'encrypted' then
        Result.Encrypted := True
      else if Key = 'requiresPassword' then
        Result.RequiresPassword := True;
      Inc(P, 4);  { true }
    end
    else if J[P] = 'f' then
      Inc(P, 5)  { false — fields already default to False }
    else if J[P] = 'n' then
      Inc(P, 4)  { null — pdfaLevel stays '' }
    else
    begin
      if Key = 'pageCount' then
        Result.PageCount := Round(JsonParseNumber(J, P))
      else
        JsonParseNumber(J, P);  { discard unknown number }
    end;
  end;
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

class function Pdf.ExtractPageText(const PdfBytes: TBytes; PageIndex: NativeUInt): string;
var
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  Check(Fpdf_extract_page_text(BytePtr(PdfBytes), Length(PdfBytes), PageIndex, P, Len));
  Result := Utf8BytesToString(TakeBuffer(P, Len));
end;

class function Pdf.ExtractImagesToDir(const PdfBytes: TBytes; const Dir: string): NativeUInt;
var
  ud: UTF8String;
  Count: NativeUInt;
begin
  EnsureLoaded;
  ud := U8(Dir);
  Count := 0;
  Check(Fpdf_extract_images_to_dir(BytePtr(PdfBytes), Length(PdfBytes), PAnsiChar(ud), Count));
  Result := Count;
end;

class function Pdf.RenderPageToPng(const PdfBytes: TBytes; PageIndex: NativeUInt; Dpi: Double): TBytes;
var
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  Check(Fpdf_render_page_to_png(BytePtr(PdfBytes), Length(PdfBytes), PageIndex, Dpi, P, Len));
  Result := TakeBuffer(P, Len);
end;

class function Pdf.PageCount(const PdfBytes: TBytes): NativeUInt;
var
  Count: NativeUInt;
begin
  EnsureLoaded;
  Count := 0;
  Check(Fpdf_page_count(BytePtr(PdfBytes), Length(PdfBytes), Count));
  Result := Count;
end;

class function Pdf.VerifySignaturesJson(const PdfBytes: TBytes): UTF8String;
var
  P: PByte;
  Len: NativeUInt;
  Raw: TBytes;
begin
  EnsureLoaded;
  Check(Fpdf_verify_signatures_json(BytePtr(PdfBytes), Length(PdfBytes), P, Len));
  Raw := TakeBuffer(P, Len);
  SetLength(Result, Length(Raw));
  if Length(Raw) > 0 then
    Move(Raw[0], Result[1], Length(Raw));
end;

class function Pdf.FindText(const PdfBytes: TBytes; const Query: string;
  CaseSensitive: Boolean): TTextHits;
var
  uq: UTF8String;
  P: PByte;
  Len: NativeUInt;
  Json: UTF8String;
  Raw: TBytes;
begin
  EnsureLoaded;
  uq := U8(Query);
  Check(Fpdf_find_text_json(BytePtr(PdfBytes), Length(PdfBytes), PAnsiChar(uq),
        Ord(CaseSensitive), P, Len));
  Raw := TakeBuffer(P, Len);
  SetLength(Json, Length(Raw));
  if Length(Raw) > 0 then
    Move(Raw[0], Json[1], Length(Raw));
  Result := ParseTextHits(Json);
end;

class function Pdf.MeasurePages(const Pdf: TBytes): TArray<TPageGeometry>;
var
  P: PByte;
  Len: NativeUInt;
  Json: UTF8String;
  Raw: TBytes;
begin
  EnsureLoaded;
  Check(Fpdf_measure_pages_json(BytePtr(Pdf), Length(Pdf), P, Len));
  Raw := TakeBuffer(P, Len);
  SetLength(Json, Length(Raw));
  if Length(Raw) > 0 then
    Move(Raw[0], Json[1], Length(Raw));
  Result := ParseGeometries(Json);
end;

class function Pdf.MeasurePage(const Pdf: TBytes; Index: Integer): TPageGeometry;
var
  Pages: TArray<TPageGeometry>;
begin
  Pages := MeasurePages(Pdf);
  if (Index < 0) or (Index >= Length(Pages)) then
    raise ERustPdf.Create(Format('page index %d out of range (0..%d)',
      [Index, Length(Pages) - 1]), psInvalidArgument);
  Result := Pages[Index];
end;

class function Pdf.Inspect(const Pdf: TBytes): TPdfOverview;
var
  P: PByte;
  Len: NativeUInt;
  Json: UTF8String;
  Raw: TBytes;
begin
  EnsureLoaded;
  Check(Fpdf_inspect_json(BytePtr(Pdf), Length(Pdf), P, Len));
  Raw := TakeBuffer(P, Len);
  SetLength(Json, Length(Raw));
  if Length(Raw) > 0 then
    Move(Raw[0], Json[1], Length(Raw));
  Result := ParseOverview(Json);
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

class function Pdf.ListSignatures(const PdfBytes: TBytes): TSignatureFields;
var
  P: PByte;
  Len: NativeUInt;
  Raw: string;
  Lines: TStringList;
  I, Tab: Integer;
begin
  EnsureLoaded;
  Check(Fpdf_list_signatures(BytePtr(PdfBytes), Length(PdfBytes), P, Len));
  Raw := Utf8BytesToString(TakeBuffer(P, Len));
  SetLength(Result, 0);
  Lines := TStringList.Create;
  try
    Lines.Text := Raw;
    for I := 0 to Lines.Count - 1 do
    begin
      if Lines[I] = '' then
        Continue;
      Tab := Pos(#9, Lines[I]);
      if Tab < 1 then
        Continue;
      SetLength(Result, Length(Result) + 1);
      Result[High(Result)].Signed := Copy(Lines[I], 1, Tab - 1) = '1';
      Result[High(Result)].Name := UTF8String(Copy(Lines[I], Tab + 1, MaxInt));
    end;
  finally
    Lines.Free;
  end;
end;

class function Pdf.BeginSigning(const PdfBytes: TBytes): TSigningSession;
begin
  Result := BeginSigning(PdfBytes, SigningOptions);
end;

class function Pdf.BeginSigning(const PdfBytes: TBytes;
  const Options: TSigningOptions): TSigningSession;
var
  C: TPdfSigningOptionsC;
  DocP, TbsP: PByte;
  DocLen, TbsLen: NativeUInt;
begin
  EnsureLoaded;
  C := MakeSignOpts(Options);
  Check(Fpdf_sign_begin(BytePtr(PdfBytes), Length(PdfBytes), @C,
        DocP, DocLen, TbsP, TbsLen));
  Result := TSigningSession.Create(TakeBuffer(DocP, DocLen), TakeBuffer(TbsP, TbsLen));
end;

class function Pdf.CompleteSignature(const Document, Container: TBytes): TBytes;
var
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  Check(Fpdf_sign_complete(BytePtr(Document), Length(Document),
        BytePtr(Container), Length(Container), P, Len));
  Result := TakeBuffer(P, Len);
end;

class function Pdf.SignWith(const PdfBytes, CertDer: TBytes;
  Callback: TPdfRemoteSign): TBytes;
begin
  Result := SignWith(PdfBytes, CertDer, Callback, [], SigningOptions);
end;

class function Pdf.SignWith(const PdfBytes, CertDer: TBytes; Callback: TPdfRemoteSign;
  const Chain: array of TBytes): TBytes;
begin
  Result := SignWith(PdfBytes, CertDer, Callback, Chain, SigningOptions);
end;

class function Pdf.SignWith(const PdfBytes, CertDer: TBytes; Callback: TPdfRemoteSign;
  const Chain: array of TBytes; const Options: TSigningOptions): TBytes;
var
  chainPtrs: array of PByte;
  chainLens: array of NativeUInt;
  I: Integer;
  C: TPdfSigningOptionsC;
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  SetLength(chainPtrs, Length(Chain));
  SetLength(chainLens, Length(Chain));
  for I := 0 to High(Chain) do
  begin
    chainPtrs[I] := BytePtr(Chain[I]);
    chainLens[I] := Length(Chain[I]);
  end;
  C := MakeSignOpts(Options);
  GActiveSigner := Callback;  { recovered by the trampoline on this thread }
  try
    Check(Fpdf_sign_with(BytePtr(PdfBytes), Length(PdfBytes), BytePtr(CertDer), Length(CertDer),
          BPtr(chainPtrs), SzPtr(chainLens), Length(Chain), @C,
          TPdfSignHashFn(@SignHashTrampoline), nil, P, Len));
  finally
    GActiveSigner := nil;
  end;
  Result := TakeBuffer(P, Len);
end;

class function Pdf.BeginTimestamp(const PdfBytes: TBytes): TTimestampSession;
var
  DocP, TbsP: PByte;
  DocLen, TbsLen: NativeUInt;
begin
  EnsureLoaded;
  Check(Fpdf_timestamp_begin(BytePtr(PdfBytes), Length(PdfBytes),
        DocP, DocLen, TbsP, TbsLen));
  Result.Document := TakeBuffer(DocP, DocLen);
  Result.Tbs := TakeBuffer(TbsP, TbsLen);
end;

class function Pdf.TimestampRequest(const Imprint: TBytes; CertReq: Boolean): TBytes;
begin
  Result := TimestampRequest(Imprint, nil, CertReq);
end;

class function Pdf.TimestampRequest(const Imprint, Nonce: TBytes; CertReq: Boolean): TBytes;
var
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  Check(Fpdf_timestamp_request(BytePtr(Imprint), Length(Imprint),
        BytePtr(Nonce), Length(Nonce), Ord(CertReq), P, Len));
  Result := TakeBuffer(P, Len);
end;

class function Pdf.TimestampTokenFromResponse(const Response: TBytes): TBytes;
var
  P: PByte;
  Len: NativeUInt;
begin
  EnsureLoaded;
  Check(Fpdf_timestamp_token_from_response(BytePtr(Response), Length(Response), P, Len));
  Result := TakeBuffer(P, Len);
end;

end.
