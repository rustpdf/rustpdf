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

procedure WriteAllBytes(const Path: string; const Data: TBytes);
var
  S: TFileStream;
begin
  S := TFileStream.Create(Path, fmCreate);
  try
    if Length(Data) > 0 then
      S.WriteBuffer(Data[0], Length(Data));
  finally
    S.Free;
  end;
end;

function BytesEqual(const A, B: TBytes): Boolean;
var
  I: Integer;
begin
  if Length(A) <> Length(B) then
    Exit(False);
  for I := 0 to High(A) do
    if A[I] <> B[I] then
      Exit(False);
  Result := True;
end;

{$IFDEF UNIX}
{ Model A signer + an independent SHA-256 oracle, both backed by the openssl
  CLI (present in this environment per the project's validator set). Guarded to
  UNIX so the binding's own test still compiles on Windows/Delphi. }
type
  TOpenSslSigner = class
    KeyPemPath: string;   { the signer key, converted to PEM once }
    WorkDir: string;
    function SignHash(const Data: TBytes): TBytes;
  end;

function TOpenSslSigner.SignHash(const Data: TBytes): TBytes;
var
  DataPath, SigPath, Cmd: string;
  Rc: Integer;
begin
  DataPath := IncludeTrailingPathDelimiter(WorkDir) + 'tbs.bin';
  SigPath := IncludeTrailingPathDelimiter(WorkDir) + 'sig.bin';
  WriteAllBytes(DataPath, Data);
  Cmd := Format('openssl dgst -sha256 -sign "%s" -out "%s" "%s"',
    [KeyPemPath, SigPath, DataPath]);
  Rc := ExecuteProcess('/bin/sh', ['-c', Cmd]);
  if Rc <> 0 then
    raise Exception.Create('openssl signing failed, rc=' + IntToStr(Rc));
  Result := ReadBytes(SigPath);
end;

function OpenSslSha256(const Data: TBytes; const WorkDir: string): TBytes;
var
  DataPath, DigPath, Cmd: string;
  Rc: Integer;
begin
  DataPath := IncludeTrailingPathDelimiter(WorkDir) + 'sha_in.bin';
  DigPath := IncludeTrailingPathDelimiter(WorkDir) + 'sha_out.bin';
  WriteAllBytes(DataPath, Data);
  Cmd := Format('openssl dgst -sha256 -binary -out "%s" "%s"', [DigPath, DataPath]);
  Rc := ExecuteProcess('/bin/sh', ['-c', Cmd]);
  if Rc <> 0 then
    raise Exception.Create('openssl dgst failed, rc=' + IntToStr(Rc));
  Result := ReadBytes(DigPath);
end;
{$ENDIF}

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
  { deferred / external (HSM) signing — issue #41 }
  Session: TSigningSession;
  SigFields: TSignatureFields;
  Opts: TSigningOptions;
  HashV: TBytes;
  Ca: TBytes;
  SignedCount: Integer;
  { issue #41 P1 — positional search + normalization }
  Hits: TTextHits;
  NormEd, VerEd: TPdfEditable;
  NormBytes, VerBytes: TBytes;
  { issue #45 P1 — measure / inspect / fill_rect / place_text }
  Geos: TArray<TPageGeometry>;
  Geo: TPageGeometry;
  RotEd, PlaceEd: TPdfEditable;
  RotBytes, PlaceBytes, DrawImgBytes: TBytes;
  Ovw: TPdfOverview;
  { issue #50 — place_text_aligned / masked_text / extract_page_text }
  AlignedBytes, MaskedBytes: TBytes;
  PageText: string;
{$IFDEF UNIX}
  Signer: TOpenSslSigner;
  KeyPemPath: string;
  ModelABytes: TBytes;
  ConvCmd: string;
{$ENDIF}
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
  { issue #41 P1: rich certificate detail fields are now present. }
  Assert(TextContains(string(SigJson), '"issuer"'), 'verify JSON has issuer');
  Assert(TextContains(string(SigJson), '"serial_number"'), 'verify JSON has serial_number');
  Assert(TextContains(string(SigJson), '"cert_count"'), 'verify JSON has cert_count');
  Assert(TextContains(string(SigJson), '"has_timestamp"'), 'verify JSON has has_timestamp');
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

  { 9. Deferred / external (HSM) signing — issue #41. }

  { 9a. ListSignatures: the locally-signed PDF has exactly one signed field. }
  SigFields := Pdf.ListSignatures(Signed);
  SignedCount := 0;
  for I := 0 to High(SigFields) do
    if SigFields[I].Signed then
      Inc(SignedCount);
  Assert(Length(SigFields) = 1, 'ListSignatures finds one field');
  Assert(SignedCount = 1, 'the field is reported signed');
  Assert(Pdf.ListSignatures(PlainBytes) = nil, 'unsigned doc has no signature fields');
  Writeln(Format('list_signatures ok (%d field, name="%s")',
    [Length(SigFields), string(SigFields[0].Name)]));

  { 9b. BeginSigning (Model B, phase 1): prepared doc + bytes-to-sign + hash. }
  Opts := SigningOptions;
  Opts.Reason := 'HSM test';
  Opts.Name := 'Edivan';
  Opts.Pades := True;
  Opts.Certify := CertifyForms;
  Session := Pdf.BeginSigning(PlainBytes, Opts);
  try
    Assert(Length(Session.Document) > 0, 'BeginSigning Document non-empty');
    Assert(Length(Session.Bytes) > 0, 'BeginSigning Bytes non-empty');
    Assert(Contains(Session.Document, '/ByteRange'), 'prepared doc has /ByteRange');
    Assert(Contains(Session.Document, 'ETSI.CAdES.detached'), 'PAdES subfilter from options');
    HashV := Session.Hash;
    Assert(Length(HashV) = 32, 'Hash is 32 bytes (SHA-256)');
  {$IFDEF UNIX}
    { Cross-validate the binding's pure-Pascal SHA-256 against openssl. }
    Assert(BytesEqual(HashV, OpenSslSha256(Session.Bytes, ImgDir)),
      'Pascal SHA-256 matches openssl');
    Writeln('begin_signing ok (hash cross-checked vs openssl)');
  {$ELSE}
    Writeln('begin_signing ok');
  {$ENDIF}
  finally
    Session.Free;
  end;

  { 9c. Model A (SignWith): library builds the CMS, an external signer (openssl,
    standing in for an HSM) produces the raw RSA signature. The private key never
    enters the library. Requires a Pascal crypto stand-in, so UNIX/openssl only. }
{$IFDEF UNIX}
  Ca := ReadBytes(Fx + 'signer_ca.der');
  KeyPemPath := IncludeTrailingPathDelimiter(ImgDir) + 'signer_key.pem';
  ConvCmd := Format('openssl pkey -inform DER -in "%s" -out "%s"',
    [Fx + 'signer_key.pk8', KeyPemPath]);
  Assert(ExecuteProcess('/bin/sh', ['-c', ConvCmd]) = 0, 'convert signer key to PEM');
  Signer := TOpenSslSigner.Create;
  try
    Signer.KeyPemPath := KeyPemPath;
    Signer.WorkDir := ImgDir;
    Opts := SigningOptions;
    Opts.Reason := 'Model A';
    { The library builds the CMS, openssl (the HSM stand-in) produces the raw RSA
      signature, the chain certs are embedded alongside the signer cert, and the
      result fully verifies. The private key never enters the library. }
    ModelABytes := Pdf.SignWith(PlainBytes, Cert, Signer.SignHash, [Ca], Opts);
    SigJson := Pdf.VerifySignaturesJson(ModelABytes);
    Assert(TextContains(string(SigJson), '"signature_valid":true'),
      'Model A signature verifies');
    Assert(TextContains(string(SigJson), 'rust-pdf test signer'),
      'Model A signer identity');
    SigFields := Pdf.ListSignatures(ModelABytes);
    Assert((Length(SigFields) = 1) and SigFields[0].Signed, 'Model A produced one signed field');
  finally
    Signer.Free;
  end;
  Writeln(Format('sign_with (Model A, external openssl signer) ok (%d bytes)',
    [Length(ModelABytes)]));
{$ELSE}
  Writeln('sign_with (Model A) skipped: needs an external signer (openssl) on UNIX');
{$ENDIF}

  { 10. Positional text search (issue #41 P1). PlainBytes shows 'segredo'. }
  Hits := Pdf.FindText(PlainBytes, 'segredo');
  Assert(Length(Hits) >= 1, 'find_text finds at least one hit');
  Assert((Hits[0].Width > 0) and (Hits[0].Height > 0), 'find_text hit has a non-empty box');
  Assert(Hits[0].Page = 0, 'find_text hit is on page 0');
  Assert(Length(Pdf.FindText(PlainBytes, 'no-such-text')) = 0, 'find_text miss -> empty');
  Writeln(Format('find_text ok (%d hit(s); first box %.1fx%.1f at %.1f,%.1f)',
    [Length(Hits), Hits[0].Width, Hits[0].Height, Hits[0].X, Hits[0].Y]));

  { 11. Normalization: strip PDF/A + set version (issue #41 P1). ConvBytes is a
    PDF/A (section 5e) carrying the XMP pdfaid identifier; Normalize removes the
    /Metadata so the A-conformance claim is gone. }
  Assert(Contains(ConvBytes, 'pdfaid'), 'PDF/A input carries the pdfaid claim');
  NormEd := TPdfEditable.Load(ConvBytes);
  try
    NormEd.Normalize(2);  { plain PDF 1.7 }
    NormBytes := NormEd.ToBytes;
  finally
    NormEd.Free;
  end;
  Assert(not Contains(NormBytes, 'pdfaid'), 'normalize strips the PDF/A pdfaid claim');
  Writeln(Format('normalize ok (%d bytes)', [Length(NormBytes)]));

  VerEd := TPdfEditable.Load(PlainBytes);
  try
    VerEd.SetVersion(3);  { PDF 2.0 header }
    VerBytes := VerEd.ToBytes;
  finally
    VerEd.Free;
  end;
  Assert(StartsWith(VerBytes, TEncoding.ASCII.GetBytes('%PDF-2.0')),
    'set_version writes a 2.0 header');
  Writeln('set_version ok');

  { 12. Page geometry (issue #45 P1). Pdfa is a single-page document. }
  Geos := Pdf.MeasurePages(Pdfa);
  Assert(Length(Geos) = 1, 'measure_pages returns one page');
  Geo := Pdf.MeasurePage(Pdfa, 0);
  Assert(Geo.Page = 0, 'page index is 0');
  Assert((Geo.Width > 0) and (Geo.Height > 0), 'page has a positive size');
  Assert(Geo.Rotation = 0, 'unrotated page reports rotation 0');
  Assert((Geo.RotatedWidth = Geo.Width) and (Geo.RotatedHeight = Geo.Height),
    'rotated size equals raw size at rotation 0');
  Assert((Geo.MediaBox.Width > 0) and (Geo.MediaBox.Height > 0), 'MediaBox has extent');
  Writeln(Format('measure ok (page 0 is %.1fx%.1f, mediaBox %.1fx%.1f)',
    [Geo.Width, Geo.Height, Geo.MediaBox.Width, Geo.MediaBox.Height]));

  { Rotate the page 90 degrees, then confirm rotatedWidth/Height swap. }
  RotEd := TPdfEditable.Load(Pdfa);
  try
    RotEd.RotatePage(0, 90);
    RotBytes := RotEd.ToBytes;
  finally
    RotEd.Free;
  end;
  Geo := Pdf.MeasurePage(RotBytes, 0);
  Assert(Geo.Rotation = 90, 'rotated page reports rotation 90');
  Assert(Geo.RotatedWidth = Geo.Height, 'rotatedWidth = unrotated height at 90');
  Assert(Geo.RotatedHeight = Geo.Width, 'rotatedHeight = unrotated width at 90');
  Writeln(Format('rotation swap ok (raw %.1fx%.1f -> rotated %.1fx%.1f)',
    [Geo.Width, Geo.Height, Geo.RotatedWidth, Geo.RotatedHeight]));

  { 13. Inspect (issue #45 P1): the PDF/A-2a doc and the encrypted doc. }
  Ovw := Pdf.Inspect(Pdfa);
  Assert(Ovw.PageCount = 1, 'inspect page count');
  Assert(not Ovw.Encrypted, 'PDF/A doc is not encrypted');
  Assert(Ovw.PdfaLevel <> '', 'inspect reports a PDF/A level');
  Assert(Ovw.Version <> '', 'inspect reports a version');
  Writeln(Format('inspect ok (version=%s, pdfa=%s, pages=%d)',
    [Ovw.Version, Ovw.PdfaLevel, Ovw.PageCount]));
  Ovw := Pdf.Inspect(EncBytes);
  Assert(Ovw.Encrypted, 'inspect detects the encrypted doc');
  Assert(Ovw.Encryption <> '', 'inspect reports an encryption method');
  Writeln(Format('inspect (encrypted) ok (encryption=%s)', [Ovw.Encryption]));

  { 14. FillRect + PlaceText (issue #45 P1). PlainBytes is a single-page doc;
    place visible text and confirm it round-trips through extraction. }
  PlaceEd := TPdfEditable.Load(PlainBytes);
  try
    Assert(PlaceEd.FillRect(0, 60, 480, 200, 40, 0.95, 0.95, 0.6, 1.0),
      'fill_rect on page 0 succeeds');
    Assert(PlaceEd.PlaceText(0, 72, 492, 'PLACED-HERE', 18, 0.0, 0.0, 0.0, 0.0),
      'place_text on page 0 succeeds');
    Assert(not PlaceEd.FillRect(99, 0, 0, 10, 10, 0, 0, 0, 1.0),
      'fill_rect on a missing page -> false');
    Assert(not PlaceEd.PlaceText(99, 0, 0, 'X', 12, 0, 0, 0, 0.0),
      'place_text on a missing page -> false');
    PlaceBytes := PlaceEd.ToBytes;
  finally
    PlaceEd.Free;
  end;
  Assert(TextContains(Pdf.ExtractText(PlaceBytes), 'PLACED-HERE'),
    'placed text is extractable');
  Writeln(Format('fill_rect + place_text ok (%d bytes)', [Length(PlaceBytes)]));

  { 15. DrawImage (issue #50). Stamp a small PNG onto an existing page and
    confirm it round-trips to a valid, larger document; a missing page -> false. }
  PlaceEd := TPdfEditable.Load(PlainBytes);
  try
    Assert(PlaceEd.DrawImage(0, TinyPng, 72, 600, 144, 144, 0.0),
      'draw_image on page 0 succeeds');
    Assert(not PlaceEd.DrawImage(99, TinyPng, 0, 0, 10, 10, 0.0),
      'draw_image on a missing page -> false');
    DrawImgBytes := PlaceEd.ToBytes;
  finally
    PlaceEd.Free;
  end;
  Assert((Length(DrawImgBytes) > Length(PlainBytes)) and
         StartsWith(DrawImgBytes, TEncoding.ASCII.GetBytes('%PDF')),
    'draw_image output is a valid, larger PDF');
  Writeln(Format('draw_image ok (%d bytes)', [Length(DrawImgBytes)]));

  { 16. place_text aligned + masked_text (issue #50). Center-aligned placed text
    and a masked (boxed) right-aligned label both round-trip through extraction. }
  PlaceEd := TPdfEditable.Load(PlainBytes);
  try
    Assert(PlaceEd.PlaceText(0, 300, 520, 'CENTERED', 16, 0.0, 0.0, 0.0, 0.0, paCenter),
      'place_text (centered) on page 0 succeeds');
    Assert(not PlaceEd.PlaceText(99, 0, 0, 'X', 12, 0, 0, 0, 0.0, paRight),
      'place_text aligned on a missing page -> false');
    AlignedBytes := PlaceEd.ToBytes;
  finally
    PlaceEd.Free;
  end;
  Assert(TextContains(Pdf.ExtractText(AlignedBytes), 'CENTERED'),
    'centered placed text is extractable');

  PlaceEd := TPdfEditable.Load(PlainBytes);
  try
    { Default text colour black, background white; right-aligned in the box. }
    Assert(PlaceEd.MaskedText(0, 60, 440, 200, 24, 'MASKED-LABEL', 14,
            0.0, 0.0, 0.0, 1.0, 1.0, 1.0, paRight),
      'masked_text on page 0 succeeds');
    Assert(not PlaceEd.MaskedText(99, 0, 0, 10, 10, 'X', 12),
      'masked_text on a missing page -> false');
    MaskedBytes := PlaceEd.ToBytes;
  finally
    PlaceEd.Free;
  end;
  Assert(TextContains(Pdf.ExtractText(MaskedBytes), 'MASKED-LABEL'),
    'masked text is extractable');
  Writeln('place_text aligned + masked_text ok');

  { 17. extract_page_text (issue #50): single-page extraction; out-of-range page
    raises psInvalidArgument. }
  PageText := Pdf.ExtractPageText(MaskedBytes, 0);
  Assert(TextContains(PageText, 'MASKED-LABEL'), 'extract_page_text returns page 0 text');
  Blocked := False;
  try
    Pdf.ExtractPageText(MaskedBytes, 99);
  except
    on E: ERustPdf do
      Blocked := E.Status = psInvalidArgument;
  end;
  Assert(Blocked, 'extract_page_text on a missing page raises psInvalidArgument');
  Writeln('extract_page_text ok');

  Writeln('OK: full Delphi/Object-Pascal binding surface exercised');
end.
