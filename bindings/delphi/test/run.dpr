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

var
  Root, Font, DevLicense, Fx: string;
  Doc, Form, Plain: TPdfDocument;
  Ed, A, B, Merged, EncEd: TPdfEditable;
  F, PF: Integer;
  Pdfa, Incr, Fb, EncBytes, Signed, Stamped, Dss, PlainBytes: TBytes;
  Buttons: TRadioButtons;
  Blocked: Boolean;
  Key, Cert, TsaKey, TsaCert: TBytes;
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

  { 6. Encryption (AES-256) round-trips. }
  Plain := TPdfDocument.Create;
  try
    PF := Plain.AddFontFile(Font);
    Plain.AddPage.ShowText(PF, 14, 72, 700, 'segredo');
    PlainBytes := Plain.ToBytes;
  finally
    Plain.Free;
  end;
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
