// Execute original ordinary-file builders with synthetic bytes and identity AES.
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const crypto = require('crypto');
const source = fs.readFileSync(path.resolve(__dirname, '../../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');
function section(start, end, offset = 0) {
    const a = source.indexOf(start, offset), b = source.indexOf(end, a);
    assert(a >= 0 && b > a, 'Original source anchors changed: ' + start);
    return source.slice(a, b);
}
const util = vm.runInNewContext('(class {' + section('static getExtension(e) {', 'static getClone(e) {') + section('static formatFileNameBeforeSend(e) {', 'static isFolder(e) {') + '})');
const info = section('getInfoUploadFileFromData(e) {', 'getInfoUploadVideoFromData(e) {');
const file = section('getFileAsyncItem(e, t, n, a, s, i, l, d) {', '\n            };\n            n.d = R');
const FileAsync = vm.runInNewContext('(class {' + info + file + '})', {
    i:{b:{getFileDomain:()=> 'https://files.example.test'}}, c:{c:{File:'file'}},
    o:{default:util}, u:{b:2}, h:{c:{convertFileNameBeforeSend:()=> {throw Error('Folders out of scope');}}},
    r:{GROUPID_PREFIX:'group_'},
});
const metadata = new FileAsync();
const chunkAnchor = source.indexOf('let T = c.name,');
const chunkStart = source.lastIndexOf('async buildSendChunksInfo(', chunkAnchor);
const chunkEnd = source.indexOf('                resendChunk(', chunkStart);
assert(chunkStart > 0 && chunkEnd > chunkStart);
class FormData { constructor() { this.entries = []; } append(key,value) { this.entries.push([key,value]); } }
class File { constructor(parts,name) { this.parts=parts.map(p=>Array.from(p)); this.name=name; } }
const Chunker = vm.runInNewContext('(class {' + source.slice(chunkStart,chunkEnd) + '})', {
    C:{getAlreadySentSetFromCheckSum:()=>new Set(),setTotalChunk(){}}, z:{a:{getZaloClientID:()=> 'synthetic-device'}},
    D:{GROUPID_PREFIX:'group_'}, g:{c:{Voice:'voice'}}, ne:new Set(), f:{a:{now:()=>100}},
    m:{d:{getInfoUploadFromData:async data=>metadata.getInfoUploadFileFromData(data),getUploadHeaders:async()=>({})}},
    s:{a:Object.assign}, r:{q:{SEND_CHUNK:'synthetic'}}, He:()=>10000, FormData, File,
    o:{default:{encodeAES:text=>text}}, i:{default:{_getCommonParams:()=> 'zpw_ver=800&zpw_type=24'}},
});
const ttlStart = source.indexOf('                d = (e, t = {}) => {', source.indexOf('        iiq4: function('));
const ttlEnd = source.indexOf('                A = (e, t = {}) => {', ttlStart);
const ttl = vm.runInNewContext('(' + source.slice(ttlStart,ttlEnd).trim().replace(/^d = /,'').replace(/,$/,'') + ')', {a:{a:Object.assign}});
const post = section('static sendMsgFileAsync(e, t, n, o) {', 'static requestCloudKey(e) {');
const build = section('static buildParams(e, t, n, s, o, r, l) {', 'static sendZText(e, t, n, a, s, o, r = null) {');
const Api = vm.runInNewContext('(class {' + build + post + '})', {
    _:{b:{getFileDomain:()=> 'https://files.example.test'}}, a:{a:Object.assign},
    D:{a:{getZaloClientID:()=> 'synthetic-device'}}, s:{default:{retryConfig:{enable_add_missing_retry:false}}},
    i:{default:{encodeAES:text=>text}}, y:{f:ttl}, B:{d:{STAGE_4_SERVER_SUBMIT:4}},
    x:{a:{getInstance:()=>({getLogger:()=>({zsymb(){}}),setLogData:()=>({}),getLogData:()=>({})})}},
});
Api.transformParams=()=>{};
Api._getCommonParams=()=> 'zpw_ver=800&zpw_type=24';
Api._post=async(url,body)=>({url,payload:JSON.parse(decodeURIComponent(body.slice(7)))});
(async()=>{
    const cases=[];
    const parts=[new Uint8Array([0,1,2,255]),new Uint8Array([10,13])];
    const checksum=crypto.createHash('md5').update(Buffer.from(parts.flatMap(p=>Array.from(p)))).digest('hex');
    for(const group of [false,true]) for(const name of ['Tài liệu.txt','Bao ca\u0301o.PDF','archive.longextension','unicode.😀😀😀😀','Tài liệu.Ả']) {
        const data={file:{name,size:6},partInfo:{partId:1,startChunkId:1},clientId:7001,toUid:group?'group_33':'22',isGroup:group,uploadMainType:'file'};
        const chunks=await new Chunker().buildSendChunksInfo(1,data,checksum,null,parts,4,2);
        const base=metadata.getFileAsyncItem('file',{file:{name},checksum,totalSize:6},'synthetic-file',7001,group,data.toUid,null,{fType:1,fileCount:0,fdata:'{}'});
        // Source __waitAsyncFileInfo assigns a string resource completion to fileUrl.
        base.fileUrl='https://files.example.test/resource';
        const final=await Api.sendMsgFileAsync(base,group,null,null);
        cases.push({group,name,checksum,chunks:chunks.map(chunk=>({url:chunk.url,payload:JSON.parse(new URL(chunk.url).searchParams.get('params')),multipart:chunk.data.entries})),final});
    }
    assert.equal(cases[0].chunks[0].payload.chunkId,1);
    assert(!('checksum' in cases[0].chunks[0].payload));
    assert.equal(cases[1].chunks[0].payload.fileName,'Bao ca\u0301o.PDF');
    assert.equal(cases[1].final.payload.fileName,'Bao cáo.PDF');
    assert.equal(cases[1].final.payload.extension,'PDF');
    assert.equal(cases[2].final.payload.extension,'');
    assert.equal(cases[3].final.payload.extension,'');
    assert.equal(cases[4].final.payload.extension,'Ả');
    assert(!('imei' in cases[5].final.payload));
    const output=JSON.stringify({provenance:'Exact original FileAsync/getInfoUploadFileFromData, ordinary buildSendChunksInfo/getUrl, getExtension/formatFileNameBeforeSend, buildParams/sendMsgFileAsync. Synthetic bytes/IDs; identity AES and no-network transport stubs. Resource completion is not final message acceptance.',cases},null,2)+'\n';
    const destination=path.join(__dirname,'file_upload.json');
    if(process.argv.includes('--write'))fs.writeFileSync(destination,output);else assert.equal(fs.readFileSync(destination,'utf8'),output);
    console.log('Original ordinary file upload oracle: '+cases.length+' DM/group/name cases');
})().catch(error=>{console.error(error);process.exitCode=1;});
