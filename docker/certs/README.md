# Certificados extras para as imagens de teste

Coloque aqui, em formato PEM e com extensão `.crt`, os certificados raiz que a
sua rede exige. É o caso de antivírus ou proxies corporativos que inspecionam
HTTPS. Sem eles, o `docker build` falha com
`SSL certificate problem: self-signed certificate in certificate chain`.

As duas imagens (`test.Dockerfile` e `test-windows.Dockerfile`) instalam todo
`*.crt` desta pasta como autoridade confiável. Os arquivos `.crt` são da sua
máquina e ficam fora do git (`.gitignore`); só este README é versionado.

No Windows, para exportar um certificado raiz do repositório do sistema (troque
o filtro pelo nome do emissor que aparece no erro):

```powershell
$c = Get-ChildItem Cert:\LocalMachine\Root | Where-Object Subject -match 'NOME DO EMISSOR' | Select-Object -First 1
$b64 = [Convert]::ToBase64String($c.RawData, 'InsertLineBreaks')
Set-Content -Encoding ascii docker\certs\rede.crt "-----BEGIN CERTIFICATE-----`n$b64`n-----END CERTIFICATE-----"
```
