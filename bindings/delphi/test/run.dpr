{ Smoke test for the RustPdf Object Pascal binding. Exercises the whole surface
  (graphics, fonts/text, PDF/A, tagging, forms, manipulation, extraction,
  encryption, signing, timestamp, DSS) plus licensing gating. Exits non-zero on
  any failed assertion. Compiles with Free Pascal (fpc) or Delphi. }
program run;

{$IFDEF FPC}
  {$MODE DELPHI}
  {$H+}
  {$CODEPAGE UTF8}
{$ENDIF}
{$APPTYPE CONSOLE}

uses
  SysUtils, Classes, RustPdf;

function RepoRoot: string;
var
  Dir, Parent: string;
  I: Integer;
begin
  Dir := ExtractFileDir(ParamStr(0));
  for I := 0 to 13 do
  begin
    if FileExists(IncludeTrailingPathDelimiter(Dir) + 'Cargo.toml') then
      Exit(Dir);
    Parent := ExtractFileDir(ExcludeTrailingPathDelimiter(Dir));
    if (Parent = '') or (Parent = Dir) then
      Break;
    Dir := Parent;
  end;
  Dir := GetCurrentDir;
  for I := 0 to 13 do
  begin
    if FileExists(IncludeTrailingPathDelimiter(Dir) + 'Cargo.toml') then
      Exit(Dir);
    Parent := ExtractFileDir(ExcludeTrailingPathDelimiter(Dir));
    if (Parent = '') or (Parent = Dir) then
      Break;
    Dir := Parent;
  end;
  Writeln('could not locate repo root');
  Halt(2);
end;

function ReadBytes(const Path: string): TBytes;
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

function ReadText(const Path: string): string;
begin
  Result := Trim(TEncoding.ASCII.GetString(ReadBytes(Path)));
end;

function BytesToRaw(const B: TBytes): RawByteString;
begin
  SetLength(Result, Length(B));
  if Length(B) > 0 then
    Move(B[0], Result[1], Length(B));
end;

function Contains(const B: TBytes; const Sub: RawByteString): Boolean;
begin
  Result := Pos(Sub, BytesToRaw(B)) > 0;
end;

{ Substring test on the UTF-8 bytes of two strings. Avoids FPC's codepage-aware
  Pos, which can miss a byte-identical match across mixed-codepage AnsiStrings. }
function TextContains(const Haystack, Needle: string): Boolean;
var
  H, N: RawByteString;
begin
  H := UTF8String(Haystack);
  N := UTF8String(Needle);
  Result := Pos(N, H) > 0;
end;

function StartsWith(const B, Prefix: TBytes): Boolean;
var
  I: Integer;
begin
  if Length(B) < Length(Prefix) then
    Exit(False);
  for I := 0 to High(Prefix) do
    if B[I] <> Prefix[I] then
      Exit(False);
  Result := True;
end;

procedure Assert(Cond: Boolean; const Msg: string);
begin
  if not Cond then
  begin
    Writeln('ASSERT FAILED: ', Msg);
    Halt(1);
  end;
end;

{ A tiny 8x8 RGBA PNG, generated once and embedded so the image-extraction
  test has a real raster to embed and pull back out. }
function TinyPng: TBytes;
const
  Bytes: array[0..74] of Byte = (
    $89, $50, $4E, $47, $0D, $0A, $1A, $0A, $00, $00, $00, $0D, $49, $48, $44,
    $52, $00, $00, $00, $08, $00, $00, $00, $08, $08, $06, $00, $00, $00, $C4,
    $0F, $BE, $8B, $00, $00, $00, $12, $49, $44, $41, $54, $78, $DA, $63, $38,
    $A1, $A1, $F1, $1F, $1F, $66, $18, $19, $0A, $00, $E7, $64, $85, $C1, $96,
    $82, $7B, $C4, $00, $00, $00, $00, $49, $45, $4E, $44, $AE, $42, $60, $82);
var
  I: Integer;
begin
  SetLength(Result, Length(Bytes));
  for I := 0 to High(Bytes) do
    Result[I] := Bytes[I];
end;

var
  Root, Font, DevLicense, Fx: string;
  Doc, Form, Plain: TPdfDocument;
  Ed, A, B, Merged, EncEd: TPdfEditable;
  F, PF: Integer;
  Pdfa, Incr, Fb, EncBytes, Signed, Stamped, Dss, PlainBytes, Png: TBytes;
  Buttons: TRadioButtons;
  Blocked: Boolean;
  Key, Cert, TsaKey, TsaCert: TBytes;
  ImgDoc: TPdfDocument;
  ImgId: Integer;
  ImgBytes: TBytes;
  ImgDir: string;
  ImgCount: NativeUInt;
  LinkDoc: TPdfDocument;
  LinkBytes, ConvBytes, WmBytes: TBytes;
  Root1, Sub: TPdfBookmark;
  FxBytes: TBytes;
  FieldsEd: TPdfEditable;
  Names: TPdfStringArray;
  HasCity: Boolean;
  I: Integer;
  PngPath: string;
  PngStream: TFileStream;
  PngBytes: TBytes;
  SigJson: UTF8String;
begin
  Root := RepoRoot;
  Font := IncludeTrailingPathDelimiter(Root) + 'assets/fonts/Roboto-Regular.ttf';
  DevLicense := ReadText(IncludeTrailingPathDelimiter(Root) +
    'crates/license/fixtures/dev_license.txt');
  Fx := IncludeTrailingPathDelimiter(Root) + 'crates/pdf/tests/fixtures/';

  Writeln('rustpdf version: ', Pdf.Version);

  { 1. Corporate features blocked without a license. The test environment must
    not set RUSTPDF_LICENSE / RUSTPDF_LICENSE_FILE (auto-activation sources). }
  Blocked := False;
  Doc := TPdfDocument.Create;
  try
    try
      Doc.Pdfa;
      Doc.AddPage;
      Doc.ToBytes;
    except
      on ERustPdf do
        Blocked := True;
    end;
  finally
    Doc.Free;
  end;
  Assert(Blocked, 'PDF/A must be blocked without a license');

  Pdf.ActivateLicense(DevLicense);
  Writeln('license activated');

  { 2. Tagged PDF/A-2a with a font, heading and justified paragraph. }
  Doc := TPdfDocument.Create;
  try
    Doc.Pdfa(palA2A).SetInfo('Olá', 'rustpdf', '', '', '');
    F := Doc.AddFontFile(Font);
    Doc.AddPage
       .ShowText(F, 20, 72, 760, 'Título', 1)
       .Paragraph(F, 12, 72, 720, 450,
         'Um parágrafo de teste. Um parágrafo de teste. Um parágrafo de teste. ' +
         'Um parágrafo de teste. Um parágrafo de teste.', paJustify);
    Pdfa := Doc.ToBytes;
  finally
    Doc.Free;
  end;
  Assert(Length(Pdfa) > 0, 'pdfa bytes');
  Assert(TextContains(Pdf.ExtractText(Pdfa), 'Título'), 'extracted text');
  Writeln(Format('built PDF/A-2a (%d bytes); extracted ok', [Length(Pdfa)]));

  { 1c. Page rendering (Pro feature; license already active). }
  Assert(Pdf.PageCount(Pdfa) = 1, 'page count');
  Png := Pdf.RenderPageToPng(Pdfa, 0, 72.0);
  Assert((Length(Png) > 8) and (Png[1] = Ord('P')) and (Png[2] = Ord('N')) and (Png[3] = Ord('G')), 'PNG header');
  Writeln(Format('rendered page 0 -> %d byte PNG', [Length(Png)]));

  { 2b. Embed a raster image, then extract every image to a temp directory. }
  ImgDoc := TPdfDocument.Create;
  try
    ImgId := ImgDoc.AddImagePng(TinyPng);
    ImgDoc.AddPage.DrawImage(ImgId, 72, 600, 144, 144);
    ImgBytes := ImgDoc.ToBytes;
  finally
    ImgDoc.Free;
  end;
  ImgDir := GetEnvironmentVariable('TMPDIR');
  if ImgDir = '' then ImgDir := GetEnvironmentVariable('TEMP');
  if ImgDir = '' then ImgDir := GetEnvironmentVariable('TMP');
  if ImgDir = '' then ImgDir := PathDelim + 'tmp';
  ImgDir := IncludeTrailingPathDelimiter(ImgDir) +
    'rustpdf_imgs_' + FormatDateTime('hhnnsszzz', Now);
  ForceDirectories(ImgDir);
  ImgCount := Pdf.ExtractImagesToDir(ImgBytes, ImgDir);
  Assert(ImgCount >= 1, 'at least one image extracted');
  Writeln(Format('extracted %d image(s) to %s', [Int64(ImgCount), ImgDir]));

  { 3. Incremental update preserves the original prefix. }
  Ed := TPdfEditable.Load(Pdfa);
  try
    Assert(Ed.PageCount = 1, 'page count');
    Ed.SetInfo('Subject', 'via FFI');
    Assert(Ed.GetInfo('Subject') = 'via FFI', 'get_info');
    Incr := Ed.ToBytesIncremental(Pdfa);
    Assert(StartsWith(Incr, Pdfa), 'incremental preserves original');
  finally
    Ed.Free;
  end;
  Writeln(Format('incremental update ok (%d bytes)', [Length(Incr)]));

  { 4. Merge + optimize. }
  A := TPdfEditable.Load(Pdfa);
  B := TPdfEditable.Load(Pdfa);
  try
    A.Merge(B).Optimize;
    Merged := TPdfEditable.Load(A.ToBytes);
    try
      Assert(Merged.PageCount = 2, 'merged page count');
    finally
      Merged.Free;
    end;
  finally
    A.Free;
    B.Free;
  end;
  Writeln('merge + optimize ok');

  { 5. AcroForm with every field type + vector graphics. }
  Form := TPdfDocument.Create;
  try
    Form.AddPage
        .FillRgb(0.9, 0.95, 1.0).Rect(60, 60, 200, 80).Fill
        .TextField('city', 0, PdfRect(120, 700, 300, 720), 'SP', 12)
        .Checkbox('ok', 0, PdfRect(120, 670, 138, 688), True)
        .Dropdown('country', 0, PdfRect(120, 610, 300, 630), ['BR', 'PT'], 0, 12);
    SetLength(Buttons, 2);
    Buttons[0].Rect := PdfRect(120, 640, 138, 658);
    Buttons[0].ExportValue := 'a';
    Buttons[1].Rect := PdfRect(160, 640, 178, 658);
    Buttons[1].ExportValue := 'b';
    Form.RadioGroup('plan', 0, Buttons, 1);
    Fb := Form.ToBytes;
  finally
    Form.Free;
  end;
  Assert(Contains(Fb, '/AcroForm'), 'AcroForm present');
  Writeln('forms + graphics ok');

  { 5b. Hyperlinks + nested bookmarks (Tier 1). }
  LinkDoc := TPdfDocument.Create;
  try
    F := LinkDoc.AddFontFile(Font);
    LinkDoc.AddPage.ShowText(F, 14, 72, 700, 'page one');
    LinkDoc.AddPage.ShowText(F, 14, 72, 700, 'page two');
    LinkDoc.LinkUri(PdfRect(72, 690, 200, 710), 'https://example.com')
           .LinkToPage(PdfRect(72, 660, 200, 680), 1, 740);
    Root1 := TPdfBookmark.Create('Chapter 1', 0);
    try
      Sub := TPdfBookmark.Create('Section 1.1', 0, 600);
      Root1.Child(Sub).Child(TPdfBookmark.Create('Chapter 2', 1));
      LinkDoc.AddBookmark(Root1);
    finally
      Root1.Free;  { frees the whole tree }
    end;
    LinkBytes := LinkDoc.ToBytes;
  finally
    LinkDoc.Free;
  end;
  Assert(Contains(LinkBytes, '/Link'), 'link annotation present');
  Assert(Contains(LinkBytes, '/Outlines'), 'outline (bookmarks) present');
  Writeln('links + bookmarks ok');

  { 5c. Factur-X / ZUGFeRD: attach an invoice XML to a PDF/A-3. }
  Doc := TPdfDocument.Create;
  try
    F := Doc.AddFontFile(Font);
    Doc.Pdfa(palA3B);
    Doc.AddPage.ShowText(F, 12, 72, 700, 'Invoice 2026-001');
    Doc.Facturx(TEncoding.UTF8.GetBytes(
      '<?xml version="1.0" encoding="UTF-8"?><CrossIndustryInvoice/>'),
      fxEN16931);
    FxBytes := Doc.ToBytes;
  finally
    Doc.Free;
  end;
  Assert(Contains(FxBytes, 'factur-x.xml'), 'Factur-X attachment present');
  Writeln(Format('Factur-X ok (%d bytes)', [Length(FxBytes)]));

  { 5d. Form manipulation: set field values, list names, flatten. }
  FieldsEd := TPdfEditable.Load(Fb);
  try
    Names := FieldsEd.FieldNames;
    HasCity := False;
    for I := 0 to High(Names) do
      if Names[I] = 'city' then
        HasCity := True;
    Assert(Length(Names) >= 4, 'field_names returns the form fields');
    Assert(HasCity, 'field_names includes "city"');
    Assert(FieldsEd.SetCheckbox('ok', False), 'set_checkbox found "ok"');
    Assert(FieldsEd.SetChoice('country', 'PT'), 'set_choice found "country"');
    Assert(FieldsEd.SetRadio('plan', 'a'), 'set_radio found "plan"');
    Assert(not FieldsEd.SetCheckbox('nope'), 'set_checkbox missing field -> false');
    FieldsEd.FlattenForms;
    Fb := FieldsEd.ToBytes;
  finally
    FieldsEd.Free;
  end;
  Writeln(Format('form fields set + flattened (%d names)', [Length(Names)]));

  { 5e. Watermark (text + image), redaction, convert-to-PDF/A. }
  { A plain document with an embedded font, reused by sections 6 and 7. }
  Plain := TPdfDocument.Create;
  try
    PF := Plain.AddFontFile(Font);
    Plain.AddPage.ShowText(PF, 14, 72, 700, 'segredo');
    PlainBytes := Plain.ToBytes;
  finally
    Plain.Free;
  end;

  { Write the embedded PNG to a file so watermark_image_file has a real path. }
  PngBytes := TinyPng;
  PngPath := IncludeTrailingPathDelimiter(ImgDir) + 'wm.png';
  PngStream := TFileStream.Create(PngPath, fmCreate);
  try
    PngStream.WriteBuffer(PngBytes[0], Length(PngBytes));
  finally
    PngStream.Free;
  end;

  FieldsEd := TPdfEditable.Load(PlainBytes);
  try
    FieldsEd.WatermarkText('CONFIDENTIAL')
            .WatermarkImageFile(PngPath, 64, 64, 0.2);
    Assert(FieldsEd.Redact(0, [PdfRect(70, 695, 200, 715)]), 'redact page 0');
    Assert(not FieldsEd.Redact(99, [PdfRect(0, 0, 10, 10)]), 'redact missing page -> false');
    WmBytes := FieldsEd.ToBytes;
  finally
    FieldsEd.Free;
  end;
  Assert(Length(WmBytes) > 0, 'watermark + redact output');
  Writeln('watermark + redaction ok');

  FieldsEd := TPdfEditable.Load(PlainBytes);
  try
    FieldsEd.ConvertToPdfa(palA2B);
    ConvBytes := FieldsEd.ToBytes;
  finally
    FieldsEd.Free;
  end;
  Assert(Contains(ConvBytes, '/OutputIntent'), 'convert_to_pdfa adds OutputIntent');
  Writeln(Format('convert_to_pdfa ok (%d bytes)', [Length(ConvBytes)]));

  { 6. Encryption (AES-256) round-trips. PlainBytes built in section 5e. }
  EncEd := TPdfEditable.Load(PlainBytes);
  try
    EncEd.Encrypt(encAES256, '', 'owner');
    EncBytes := EncEd.ToBytes;
  finally
    EncEd.Free;
  end;
  Assert(Contains(EncBytes, '/AESV3'), 'AES-256 marker');
  Assert(TextContains(Pdf.ExtractText(EncBytes), 'segredo'), 'decrypted text');
  Writeln('encryption ok');

  { 7. Digital signature (PAdES-B-B) with the committed test key. }
  Key := ReadBytes(Fx + 'signer_key.pk8');
  Cert := ReadBytes(Fx + 'signer_cert.der');
  Signed := Pdf.Sign(PlainBytes, Key, Cert, 'Aprovado', '', '', True);
  Assert(Contains(Signed, '/ByteRange'), 'signature ByteRange');
  Writeln(Format('signed ok (%d bytes)', [Length(Signed)]));

  { 7b. Verify signatures -> raw JSON (no bundled JSON parser in Object Pascal). }
  SigJson := Pdf.VerifySignaturesJson(Signed);
  Assert(TextContains(string(SigJson), '"sub_filter"'), 'verify JSON has sub_filter');
  Assert(TextContains(string(SigJson), '"byte_range"'), 'verify JSON has byte_range');
  Assert(TextContains(string(SigJson), '"is_valid"'), 'verify JSON has is_valid');
  Assert(Pdf.VerifySignaturesJson(PlainBytes) = '[]', 'unsigned doc -> empty JSON array');
  Writeln('verify_signatures (JSON) ok');

  { 8. Document timestamp (/DocTimeStamp) + DSS (/DSS), PAdES-B-LT(A). }
  TsaKey := ReadBytes(Fx + 'tsa_key.pk8');
  TsaCert := ReadBytes(Fx + 'tsa_cert.der');
  Stamped := Pdf.Timestamp(Signed, TsaKey, TsaCert, '');
  Assert(Contains(Stamped, '/DocTimeStamp'), 'timestamp /DocTimeStamp');
  Dss := Pdf.AddDss(Signed, [Cert], []);
  Assert(Contains(Dss, '/DSS'), 'DSS /DSS');
  Writeln('timestamp + DSS ok');

  Writeln('OK: full Delphi/Object-Pascal binding surface exercised');
end.
