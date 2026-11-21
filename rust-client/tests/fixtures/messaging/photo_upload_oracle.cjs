// Execute the bundled photo builders, route selection, resource assignment and final submission.
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
const info = section('getInfoUploadPhotoFromData(e) {', 'async getInfoUploadZCloudFileFromData(e, t) {');
const settings={file_photo_limit:{enable_new_endpoint_upload_photo:false}};
const file = section('getFileAsyncItem(e, t, n, a, s, i, l, d) {', '\n            };\n            n.d = R');
const FileAsync = vm.runInNewContext('(class {' + info + file + '})', {
    i:{b:{getFileDomain:()=> 'https://files.example.test',getPhotoDomain:()=> 'https://photos.example.test',getPhotoOriginalDomain:()=> 'https://original.example.test'}}, c:{c:{File:'file',Photo:'photo'},a:{SendMsg:'send'}}, A:{default:settings}, _:{c:{HD:'hd',ORIGINAL:'original'}},
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
    m:{d:{getInfoUploadFromData:async data=>metadata.getInfoUploadPhotoFromData(data),getUploadHeaders:async()=>({})}},
    s:{a:Object.assign}, r:{q:{SEND_CHUNK:'synthetic'}}, He:()=>10000, FormData, File,
    o:{default:{encodeAES:text=>text}}, i:{default:{_getCommonParams:()=> 'zpw_ver=800&zpw_type=24'}},
});
const ttlStart = source.indexOf('                d = (e, t = {}) => {', source.indexOf('        iiq4: function('));
const ttlEnd = source.indexOf('                A = (e, t = {}) => {', ttlStart);
const ttl = vm.runInNewContext('(' + source.slice(ttlStart,ttlEnd).trim().replace(/^d = /,'').replace(/,$/,'') + ')', {a:{a:Object.assign}});
const post = section('static sendMsgPhotoAsync(e, t, n, o) {', 'static createOneOneGroupTopic(e) {');
const assignResource = section('__waitAsyncFileInfo(e, t, n, a, s) {','__logQos(e, t, n) {');
let resource;
const PhotoWait=vm.runInNewContext('(class {'+assignResource+'})',{o:{d:{getFileIdData:()=>resource}},_:{c:{Photo:'photo',File:'file',Video:'video',Voice:'voice',PhotoThumb:'thumb',VideoThumb:'vthumb'}},h:{default:{}},setTimeout:()=>1,clearTimeout(){}});
const build = section('static buildParams(e, t, n, s, o, r, l) {', 'static sendZText(e, t, n, a, s, o, r = null) {');
const Api = vm.runInNewContext('(class {' + build + post + '})', {
    _:{b:{getFileDomain:()=> 'https://files.example.test'}}, a:{a:Object.assign},
    D:{a:{getZaloClientID:()=> 'synthetic-device'}}, s:{default:{retryConfig:{enable_add_missing_retry:false}}},
    i:{default:{encodeAES:text=>text,safeParseJson:text=>text ? JSON.parse(text) : null}}, y:{f:ttl}, w:{a:{getCurrentLanguageName:()=> 'VN'}},zconsole:{zsymb(){}}, B:{d:{STAGE_4_SERVER_SUBMIT:4}},
    j:{c:{ORIGINAL:'original'}},
    x:{a:{getInstance:()=>({getLogger:()=>({zsymb(){}}),setLogData:()=>({}),getLogData:()=>({})})}},
});
Api.transformParams=()=>{};
Api._getCommonParams=()=> 'zpw_ver=800&zpw_type=24';
Api._post=async(url,body)=>({url,payload:JSON.parse(decodeURIComponent(body.slice(7)))});
(async()=>{
    const cases=[];
    const parts=[new Uint8Array([0,1,2,255]),new Uint8Array([10,13])];
    const checksum=crypto.createHash('md5').update(Buffer.from(parts.flatMap(p=>Array.from(p)))).digest('hex');
    for(const group of [false,true]) for(const route of ['file','photo']) for(const caption of ['', 'Ảnh thử nghiệm tổng hợp']) {
        settings.file_photo_limit.enable_new_endpoint_upload_photo=route==='photo';
        const data={feature:'send',photoResolutionMode:'hd',file:{name:'photo.jpg',size:6},partInfo:{partId:1,startChunkId:1},clientId:7001,toUid:group?'group_33':'22',isGroup:group,uploadMainType:'photo',jxlStatus:0};
        const chunks=await new Chunker().buildSendChunksInfo(1,data,checksum,null,parts,4,2);
        const base=metadata.getFileAsyncItem('photo',{totalSize:6},'synthetic-photo',7001,group,data.toUid,null,{title:caption,width:640,height:480});
        resource={org:'https://photos.example.test/original',thumb:'https://photos.example.test/thumb',hd:'https://photos.example.test/hd',thumbSize:1,fileSize:6,hdSize:4};
        const waiter=new PhotoWait();
        Object.assign(waiter,{_makeHandlerAsync:{},_asyncTimeout:{},_replayRequest:{}});
        const ready=await waiter.__waitAsyncFileInfo('photo',base,group,null,false);
        const final=await Api.sendMsgPhotoAsync(ready.fileAsync,group,null,null);
        cases.push({group,route,caption,checksum,width:640,height:480,urls:resource,chunks:chunks.map(chunk=>({url:chunk.url,payload:JSON.parse(new URL(chunk.url).searchParams.get('params')),multipart:chunk.data.entries})),final});
    }
    assert.equal(cases[0].chunks[0].payload.chunkId,1);
    assert(!('checksum' in cases[0].chunks[0].payload));
    assert(!('jxl' in cases[0].chunks[0].payload));
    assert.equal(cases[0].final.payload.language,'VN');
    assert.equal(cases[0].final.payload.imei,'synthetic-device');
    assert.equal(cases[4].final.payload.imei,'synthetic-device');
    assert.equal(cases[0].final.payload.normalUrl,resource.org);
    assert.equal(cases[4].final.payload.oriUrl,resource.org);
    for (const [group, scope, type] of [[false, 'message', 2], [true, 'group', 11]]) {
        const sourceRoute = metadata.getInfoUploadPhotoFromData({
            feature: 'send', photoResolutionMode: 'original', isGroup: group,
        });
        assert.equal(sourceRoute.domain, 'https://original.example.test/api/' + scope + '/zcloud-upload/photo?');
        assert.equal(sourceRoute.type, type);
        const ordinary = cases[group ? 4 : 0].final.payload;
        const final = await Api.sendMsgPhotoAsync(ordinary, group, {photoResolutionMode: 'original'}, null);
        assert.equal(new URL(final.url).pathname, '/api/' + scope + '/photo_original/send');
        assert.equal(final.payload.jcp, '{"is_original":1}');
        assert.equal(ordinary.jcp, undefined);
    }
    const output=JSON.stringify({provenance:'Exact original getInfoUploadPhotoFromData HD routing, ordinary buildSendChunksInfo/getUrl, getFileAsyncItem Photo, __waitAsyncFileInfo resource assignment, buildParams/sendMsgPhotoAsync. Synthetic bytes/IDs; identity AES and no-network transport stubs. Resource completion is not final message acceptance.',cases},null,2)+'\n';
    const destination=path.join(__dirname,'photo_upload.json');
    if(process.argv.includes('--write'))fs.writeFileSync(destination,output);else assert.equal(fs.readFileSync(destination,'utf8'),output);
    console.log('Bundled photo oracle: '+cases.length+' HD cases and 2 original-photo routes');
})().catch(error=>{console.error(error);process.exitCode=1;});
